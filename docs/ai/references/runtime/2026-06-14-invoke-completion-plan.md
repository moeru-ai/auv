# C1 Completion Plan: Finish the invoke Registry Boundary

Date: 2026-06-14

Status: proposed execution plan for finishing C1 after C1a–c. Owner chose
"complete full C1 before commit." This plan makes that concrete, flags two
entanglements, and is written for Mac-side execution (the planning sandbox is
Linux and cannot build the macOS-targeted crates, so it cannot run `cargo`).

Inputs:

- `docs/ai/references/runtime/2026-06-13-core-roadmap.md` (C1 section + exit criteria)
- `docs/ai/references/invoke-cli/2026-06-11-cli-invoke-driver-console-design.md` (namespace model)
- `docs/ai/references/invoke-cli/2026-06-14-invoke-registry-handoff.md` (C1a–c state; app-probe fix)

## Honest Scope Read First

The roadmap's C1 exit criteria bundle two different kinds of work:

1. **Rename completion** — move every remaining legacy id to a canonical
   namespace, update consumers, expand discovery. Mechanical-ish, validatable,
   reviewable. Call this **C1d**.
2. **Structural extraction** — delete `src/catalog.rs` and stop `Runtime` from
   owning the registry. This is an execution-path refactor that the invoke-console
   design **explicitly deferred** ("It should not generate execution logic in the
   first slice... Execution remains registered through... the temporary legacy
   driver adapter") and that **overlaps C2** (`auv-tracing-driver` shrinks
   `Runtime` anyway). Call this **C1e**.

**DECISION (locked 2026-06-14, owner delegated "you pick"): do C1d now; defer C1e
into C2's `Runtime` shrink.** C1d satisfies the "no `debug.*`/`verify.*` ids" half
of C1 and is committable/validatable/reviewable on its own. C1e (delete
`catalog.rs`, pull the registry out of `Runtime`) is the execution-path refactor
the design deferred and that C2 performs anyway, so it does not go in this slice.
The C1e inventory below is retained only as the handoff into C2.

## Canonical Rename Map

Already done in C1c (10): `display.list/capture/identifyPoint/projectScreenshotPoint`,
`window.list/capture/captureAxTree/findText/clickText`, `mediaControl.nowPlaying`.

Clean renames remaining (no semantics change, just id):

```text
# screen.*
debug.findScreenText        -> screen.findText
debug.waitForScreenText     -> screen.waitForText
debug.findScreenRows        -> screen.findRows
debug.waitForScreenRows     -> screen.waitForRows
debug.clickScreenText       -> screen.clickText
debug.clickScreenRow        -> screen.clickRow

# window.*
debug.waitForWindowText     -> window.waitForText
debug.findWindowRows        -> window.findRows
debug.waitForWindowRows     -> window.waitForRows
debug.observeWindowRegion   -> window.observeRegion
debug.findIconMatch         -> window.findIconMatch
debug.scrollWindowRegion    -> window.scrollRegion
debug.clickWindowRow        -> window.clickRow
verify.axText               -> window.verifyText

# input.*
debug.focusTextInput        -> input.focusText
debug.pressButton           -> input.pressButton
debug.axPressButton         -> input.axPressButton
debug.axFocusTextInput      -> input.axFocusText
debug.axClickWindowText     -> input.axClickWindowText
debug.smartPress            -> input.smartPress
debug.typeText              -> input.typeText
debug.pasteTextPreserveClipboard -> input.pasteText
debug.pressKey              -> input.key
debug.clickPoint            -> input.clickPoint
debug.clickWindowPoint      -> input.clickWindowPoint
debug.teachClick            -> input.teachClick
debug.scrollPoint           -> input.scrollPoint

# app.*
debug.activateApp           -> app.activate
debug.probePermissions      -> app.probePermissions

# overlay.*  (drop the redundant "overlay" infix)
debug.overlayShowCursor       -> overlay.showCursor
debug.overlayShowDualCursor   -> overlay.showDualCursor
debug.overlayApplyCursorBatch -> overlay.applyCursorBatch
debug.overlaySetCursor        -> overlay.setCursor
debug.overlayMoveCursor       -> overlay.moveCursor
debug.overlayMoveCursorById   -> overlay.moveCursorById
debug.overlayFlashCursor      -> overlay.flashCursor
debug.overlayFlashCursorById  -> overlay.flashCursorById
debug.overlayHideCursorId     -> overlay.hideCursorId
debug.overlayHideCursor       -> overlay.hideCursor
debug.overlayShutdown         -> overlay.shutdown
```

Previously-ambiguous ids — now LOCKED:

```text
debug.captureRegion            -> display.captureRegion   (region of a display surface)
debug.probeCoordinateReadiness -> display.probeCoordinateReadiness   (display-coordinate diagnostic)
debug.findImageText            -> screen.findImageText   (same OCR match, over an artifact not live)
debug.overlayClickPoint        -> input.overlayClickPoint   (it delivers a REAL click [Pointer disturbance];
                                  it is an input action with overlay viz, NOT visual-only — keeps overlay.* clean)
debug.fixtureObserve           -> fixture.observe   (test fixture; KEEP resolvable, but EXCLUDE from
                                  invoke_discovery_catalog — it is not a user capability)
```

Domain commands (fate — now LOCKED):

```text
music.search.results / music.result.play / music.validate.candidate.liveness
  -> KEEP in the catalog for now; EXCLUDE from invoke_discovery_catalog. The design
     wants them removed from invoke, but the auv-qqmusic frontend does not exist yet,
     so deletion would drop a capability. Their removal is a separate slice gated on
     the product-crate move. Consequence: the "no music.* in registry" exit clause is
     explicitly NOT part of C1d; C1d only guarantees no music.* in discovery/help.
recognition.read.ratio  -> KEEP, exclude from discovery; fate deferred to its own slice.
steam.library.list.v0   -> KEEP. C3 keeper; already canonical-form. (C1d does not touch it.)
```

Net: after C1d the production catalog carries only canonical
`display.*/screen.*/window.*/input.*/app.*/overlay.*/mediaControl.*` ids plus the
non-discovery survivors (`fixture.observe`, `music.*`, `recognition.read.ratio`,
`steam.library.list.v0`). No `debug.*` and no `verify.*` remain anywhere.

## Consumer-Update Inventory (the part that hides regressions)

Command ids are string-keyed, so a missed call site compiles clean and fails at
run time — exactly the `app probe` bug just caught. Every non-catalog reference to
a renamed id must change in lockstep:

```text
src/app/mod.rs        probe_app_into_run: debug.probePermissions -> app.probePermissions,
                      debug.probeCoordinateReadiness -> (decided id),
                      debug.activateApp -> app.activate,
                      debug.observeWindowRegion -> window.observeRegion
                      (the other 4 were fixed for the regression already)
src/app/tests.rs      APP_PROBE_COMMAND_IDS must be updated in the SAME edit
src/scroll_scan/mod.rs:650  command_id "debug.observeWindowRegion" -> window.observeRegion
src/scroll_scan/mod.rs:668  command_id "debug.scrollWindowRegion"  -> window.scrollRegion
src/catalog.rs        promote the CommandSpec ids + add the new constants;
                      expand invoke_discovery_catalog() to the full canonical set
src/cli.rs            help_text() NOTES lines (incl. the stale L250 verify.musicNowPlaying note)
src/runtime.rs        any test fixtures using old ids
```

Cosmetic but should land with the rename (non-crashing, hands users dead ids):

```text
hint strings across src/driver/macos/** and
crates/auv-driver-macos/src/support/selector.rs that say
`inspect debug.listWindows`, `debug.findWindowText`, `Try debug.clickWindowText`, etc.
```

Confirm-not-touch (distinct concept, not command-id resolution):

```text
src/driver/macos/observe.rs:72  VERIFY_MUSIC_NOW_PLAYING_OPERATION_ID
  -> recording/operation-id label, NOT a catalog command id. Leave unless proven
     to be resolved as a command id.
```

## C1e (only if doing structural extraction now; else defer to C2)

```text
src/runtime.rs:15,32,40,47-49  Runtime holds `commands: CommandCatalog`
src/runtime.rs:61             list_commands()
src/runtime.rs:373            invoke_direct_command_in_span resolves id -> driver/op via catalog
src/lib.rs:31,47              builds default_command_catalog() for the default Runtime
```

Removing the catalog from `Runtime` means `invoke` must receive an
already-resolved command descriptor (id -> driver_id/operation/disturbance) from
the invoke boundary instead of resolving internally. That is the execution-path
change the design deferred and that C2 also needs. Strong recommendation: do this
with C2, not inside C1.

## Validation Gate (Mac-side, must all pass before commit)

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run -- invoke --help            # full canonical index, no debug.*/verify.*
cargo run -- invoke <renamed-id> --help
auv-cli app probe <bundle-id>         # the smoke that was skipped before; must complete
cargo run -- invoke debug.typeText    # a renamed id must now FAIL (no alias execution)
```

Add two guard tests so the next rename cannot silently break a consumer:

1. extend `app_probe_command_ids_resolve_in_default_catalog` (already added) — keep
   `APP_PROBE_COMMAND_IDS` synced with `probe_app_into_run`.
2. add an equivalent assertion for the `scroll_scan` command ids resolving in
   `default_command_catalog()`.
3. a catalog test asserting no production id starts with `debug.` or `verify.`
   (this is the machine check for the C1d half of the exit criteria).

## Execution / Ownership

Claude Code executes on the Mac (it can validate iteratively; this sandbox cannot
build macOS crates). This planner can: produce/refine this map, review the
resulting diff + behavior, and do the non-Rust pieces (handoff status sync,
`.gitignore` for `.tmp-*`, doc updates) on request. Do not `git add .` — the
~10 untracked `.tmp-*` run dirs are not gitignored.

## Decisions (locked 2026-06-14, owner delegated "you pick")

1. Scope = **C1d now; C1e folded into C2.** No `catalog.rs` deletion or `Runtime`
   registry extraction in this slice.
2. The 5 previously-ambiguous id mappings: locked as above.
3. `music.*` / `recognition.read.ratio`: kept, excluded from discovery; their
   removal/rehoming is out of C1d.

No open decisions remain; this is a ready-to-execute spec.

## Doc Housekeeping (do at C1d commit time, not started)

- Update the C1 section status in `2026-06-13-auv-core-lane-roadmap.md`: C1e moved
  into C2; C1 = the rename half.
- Update `2026-06-14-invoke-registry-handoff.md` status line (regression already
  fixed in tree; "C1a–c done").

## Per-Slice Process

Standard block (`fmt --check`, `check`, `test`, `build`, `diff --check`) plus the
`app probe` real-app smoke, run ids recorded. State what changed and what was
validated, then stop for owner selection.
