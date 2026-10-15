# Invoke stdout audit and primary-result repair

**Date:** 2026-07-29  
**Status:** implemented and live-verified  
**Scope:** approved CLI output/catalog correction

## Evidence audit

All 76 commands advertised by the pre-change `auv invoke --help` were audited.
Read-only, capture, OCR, permission, media-state, and hermetic scan paths were
executed. Commands that click, type, mutate playback, buy/sell game state, or
modify a document were limited to safe validation paths; source-based success
assessment was labeled separately.

| Group | Commands audited | Main evidence |
|---|---:|---|
| DISPLAY / SCREEN / WINDOW | 25 | Live list/capture/OCR output, capture record and file lookup, safe click validation |
| INPUT / APP / OVERLAY / MEDIA CONTROL | 33 | Live permission/media reads, safe validation, direct stub probes |
| SCAN / BALATRO / TEXTEDIT | 18 | Hermetic scan success, safe game/document validation |
| **Total** | **76** | Every advertised command had command help plus at least one safe probe |

The audit found:

- 33 advertised handlers immediately panicked through `unimplemented!`.
- `display.capture`, `screen.captureRegion`, and `window.capture` persisted PNGs
  but returned neither the artifact URI nor physical path.
- `scan.frame` and `scan.coverage` hid their staged primary artifacts.
- all six `mediaControl.*` commands had typed JSON results but no corresponding
  human result report; live `nowPlaying` reproduced the blank success block.

## Implemented contract

`InvokeCommandOutput` now carries the `ArtifactMetadata` receipts that are part
of the direct command result. `InvokeResult` renders those receipts in both
human and JSON modes. The root CLI retains its concrete `FileTracingStore` and
uses the store-owned artifact path projection to add a directly openable local
path. Commands do not reconstruct or depend on the file-store directory layout.

Human output shape:

```text
  Artifacts
  auv.driver.display_capture: /absolute/store/path/<artifact-id>.png
  Artifact URI: auv://runs/<run-id>/artifacts/<artifact-id>
```

JSON adds a top-level `artifacts` array containing the durable metadata and an
optional `file_path` supplied by a file-backed frontend.

The following direct outputs attach artifact receipts:

| Command | Direct artifacts |
|---|---|
| `display.capture` | display PNG |
| `screen.captureRegion` | region PNG |
| `window.capture` | window PNG |
| `scan.frame` | frame PNG and typed frame JSON |
| `scan.coverage` | typed coverage JSON |

All 33 immediate-panic commands were **unregistered** from the default registry,
help index, and MCP adapter catalog. Their command descriptors and
`unimplemented!` handler stubs were not deleted. They remain at the original
code seams with explicit TODOs, so an implementation can replace each stub and
then restore its registration with behavioral evidence.

## Unregistered `unimplemented!` commands

Before the repair, every command below exited with code 101, wrote no stdout,
and wrote an `unimplemented: <command-id>` panic to stderr. “Unregistered” means
only that it is no longer advertised as executable by the default CLI/MCP
catalog; it does not mean that its descriptor or stub source was deleted.

| Group | Command | Pre-repair stdout | Pre-repair result | Current source status |
|---|---|---|---|---|
| DISPLAY | `display.projectScreenshotPoint` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| DISPLAY | `display.identifyPoint` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| SCREEN | `screen.findRows` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| SCREEN | `screen.waitForRows` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| SCREEN | `screen.findImageText` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| SCREEN | `screen.clickRow` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.captureAxTree` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.findRows` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.waitForRows` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.observeRegion` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.findIconMatch` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.scrollRegion` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.verifyText` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| WINDOW | `window.clickRow` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| INPUT | `input.pressButton` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| INPUT | `input.axPressButton` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| INPUT | `input.axClickWindowText` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| INPUT | `input.smartPress` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| INPUT | `input.clickPoint` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| INPUT | `input.teachClick` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| INPUT | `input.scrollPoint` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.clickPoint` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.showCursor` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.showDualCursor` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.applyCursorBatch` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.setCursor` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.moveCursor` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.moveCursorById` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.flashCursor` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.flashCursorById` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.hideCursorId` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.hideCursor` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |
| OVERLAY | `overlay.shutdown` | empty | panic / exit 101 | Descriptor and `unimplemented!` stub retained; registration removed |

Media-control human reports now expose:

- `nowPlaying`: state and available title, artist, album, source, elapsed, and
  duration values;
- mutation commands: requested command, verification status, and bounded
  before/after state summaries.

Input and activation commands now expose the evidence boundary directly:

- typed input delivery reports use `Delivery: delivered` together with
  `Verification: delivery_only`;
- dry runs use `Delivery: not_performed` together with
  `Verification: validation_only`;
- AX focus reports the resolved target, query or candidate, AX path, role, and
  delivery method without claiming that focus was read back;
- `app.activate` returns a shared `ApplicationActivationResult` and compares
  the requested bundle identifier with the post-settle foreground observation.
  Human, JSON, and MCP output use the same canonical verification status.
  (Migration notice, 2026-10-12: the result is now keyed by
  `requested_application_id` with an `application_id_kind`, and the observed
  field is `observed_application_id`; the status strings are unchanged.)

## Live verification

| Command | Observed result |
|---|---|
| `target/debug/auv invoke --help` | 28 commands; no immediate-panic or Balatro IDs advertised |
| `target/debug/auv invoke display.capture` | Absolute PNG path and URI printed; file exists |
| `target/debug/auv invoke display.capture --json` | Parseable artifact metadata plus `file_path` |
| `target/debug/auv invoke screen.captureRegion ...` | Absolute PNG path and URI printed |
| `target/debug/auv invoke window.capture --target com.microsoft.VSCode` | Absolute PNG path and URI printed |
| `target/debug/auv invoke scan.frame ...` | PNG and frame JSON paths/URIs printed |
| `target/debug/auv invoke scan.coverage ...` | Coverage JSON path/URI printed |
| `target/debug/auv invoke mediaControl.nowPlaying` | Current media state rendered in human stdout |
| `target/debug/auv invoke app.activate --target com.microsoft.VSCode` | Requested target and independently observed foreground both reported as `com.microsoft.VSCode`; verification is `verified_foreground` |
| safe input/OCR click `--dry-run` probes | `Delivery: not_performed` and `Verification: validation_only`; no input was delivered |

## Final advertised-command re-audit

The repaired help index advertises 28 commands. Every advertised command has a
non-panicking handler and a primary human-output contract. The original exact
stdout/stderr captures for all 76 pre-repair commands remain in
`docs/notes/neko/invoke-output-audit/{visual,actions,domain}.md`; the table below
records the final disposition of every command that remains public.

The initial audit included 15 `game.balatro.*` commands because the product
registry exposed them at that time. The owner subsequently removed that public
surface: `auv-cli` no longer registers the Balatro group, exposes Balatro MCP
adapters, compiles the Balatro CLI integration module, or depends on
`auv-game-balatro`. The standalone experimental `auv-game-balatro` crate remains
outside the product invoke catalog.

| Command | Primary stdout contract | Evidence boundary | Final disposition |
|---|---|---|---|
| `display.capture` | Display geometry plus directly openable PNG path and artifact URI | Live capture | Complete |
| `display.list` | Bounded display inventory and normalized geometry | Live read | Complete |
| `screen.captureRegion` | Region geometry plus directly openable PNG path and artifact URI | Live capture | Complete |
| `screen.findText` | OCR matches, text, confidence, bounds, and action point | Live read | Complete |
| `screen.waitForText` | Poll result and the resolved OCR matches | Live read | Complete |
| `screen.clickText` | OCR resolution, click point, typed delivery path and disturbance | `delivery_only`; dry run is `validation_only` | Complete and explicitly bounded |
| `window.list` | Bounded window inventory, identity, and normalized geometry | Live read | Complete |
| `window.capture` | Window identity/geometry plus directly openable PNG path and artifact URI | Live capture | Complete |
| `window.findText` | Resolved window plus OCR matches and action points | Live read | Complete |
| `window.waitForText` | Poll result, resolved window, and OCR matches | Live read | Complete |
| `window.clickText` | Window/OCR resolution, point, typed delivery path and disturbance | `delivery_only`; dry run is `validation_only` | Complete and explicitly bounded |
| `input.focusText` | Target, selector, resolved AX path/role, and AX delivery | `delivery_only`; no focus readback | Complete and explicitly bounded |
| `input.axFocusText` | Target, selector, resolved AX path/role, and AX delivery | `delivery_only`; no focus readback | Complete and explicitly bounded |
| `input.typeText` | Typed keyboard delivery, attempts, fallback, and disturbance | `delivery_only`; dry run is `validation_only` | Complete and explicitly bounded |
| `input.pasteText` | Typed paste delivery, attempts, fallback, clipboard restoration/disturbance | `delivery_only`; dry run is `validation_only` | Complete and explicitly bounded |
| `input.key` | Key plus typed keyboard delivery, attempts, fallback, and disturbance | `delivery_only`; dry run is `validation_only` | Complete and explicitly bounded |
| `input.clickWindowPoint` | Resolved window/point plus typed mouse delivery and disturbance | `delivery_only`; dry run is `validation_only` | Complete and explicitly bounded |
| `app.probePermissions` | Screen recording, accessibility, and automation statuses | Live read | Complete |
| `app.activate` | Requested target, request result, canonical verification, and observed foreground/detail | Live WindowServer observation; no screenshot proxy | Complete |
| `mediaControl.nowPlaying` | State and available title, artist, album, source, elapsed, and duration | Live read | Complete |
| `mediaControl.play` | Requested command, verification, and bounded before/after state | Backend observation | Complete |
| `mediaControl.pause` | Requested command, verification, and bounded before/after state | Backend observation | Complete |
| `mediaControl.togglePlayPause` | Requested command, verification, and bounded before/after state | Backend observation | Complete |
| `mediaControl.next` | Requested command, verification, and bounded before/after identity/state | Backend observation | Complete |
| `mediaControl.previous` | Requested command, verification, and bounded before/after identity/state | Backend observation | Complete |
| `scan.frame` | Frame summary plus directly openable PNG and typed frame JSON paths/URIs | Hermetic live fixture | Complete |
| `scan.coverage` | Coverage summary plus directly openable typed coverage JSON path/URI | Hermetic live fixture | Complete |
| `app.textedit.document.write` | Action list plus requested verification and observed semantic match when enabled | App-owned optional AX verification | Complete contract; live document mutation not performed |

Generic input commands intentionally stop at delivery evidence because they do
not accept an application-specific success predicate. A later app/domain
operation may verify its own expected state, but an unrelated screenshot is not
promoted to semantic success.
