# C1d invoke Registry Rename — Handoff

Date: 2026-06-14
Status: **historical C1d handoff; C1d completed and pushed, C2c/C2d later completed locally and validated; next slice is C2e**
Collabi sessions:
- `auv-core-c1-invoke-registry-boundary` (C1a–c historical lane)
- `auv-core-c1d-full-rename-boundary` (current startup gate + implementation lane)
Roadmap anchor: `docs/ai/references/runtime/2026-06-13-core-roadmap.md`
Design intent: `docs/ai/references/invoke-cli/2026-06-11-cli-invoke-driver-console-design.md`
Plan file: `/Users/liuziheng/.claude/plans/prancy-twirling-hopper.md`

## Locked scope for this slice

This is **C1d only**:
- finish canonical rename of remaining `debug.*` / `verify.*` production invoke ids
- expand canonical `invoke --help` / `InvokeDiscoveryCatalog`
- keep survivor commands executable but hidden from discovery
- update string-keyed consumers and stale user-facing hint strings

Explicitly **NOT in scope**:
- no `src/catalog.rs` deletion
- no Runtime registry extraction
- no execution routing changes
- no alias table
- no music / recognition / steam semantic changes
- no silent C1e → C2 merge

## Survivor policy (locked)

Old-id negative set is **only** `debug.*` / `verify.*` command ids.

Survivors remain resolvable and hidden from canonical discovery/help:
- `music.validate.candidate.liveness`
- `music.search.results`
- `music.result.play`
- `recognition.read.ratio`
- `steam.library.list.v0`
- `fixture.observe`

Special case approved during planning:
- `debug.fixtureObserve` → `fixture.observe`
- this is the only survivor rename, solely to eliminate the last production `debug.` prefix
- behavior/driver/namespace unchanged; still hidden from discovery

## What landed in the working tree

### Earlier C1a–c base
Already present before this C1d pass:
- `list-commands` tombstone
- `invoke --help` metadata path
- `InvokeDiscoveryCatalog`
- first 10 canonical ids:
  - `display.list`
  - `display.capture`
  - `display.identifyPoint`
  - `display.projectScreenshotPoint`
  - `window.list`
  - `window.capture`
  - `window.captureAxTree`
  - `window.findText`
  - `window.clickText`
  - `mediaControl.nowPlaying`
- `app probe` blocking regression from C1c was already fixed before this C1d pass

### C1d completion in this pass
Changed files in this pass:
- `src/catalog.rs`
- `src/app/mod.rs`
- `src/app/tests.rs`
- `src/scroll_scan/mod.rs`
- `src/cli.rs`
- `src/driver/macos/support/typed_capture.rs`
- `src/driver/macos/control/window_ocr.rs`
- `src/driver/macos/control/ax.rs`
- `crates/auv-driver-macos/src/support/selector.rs`
- `src/driver/macos/tests.rs`

#### `src/catalog.rs`
Completed:
- large canonical id constant block for the remaining C1d targets
- `render_invoke_help_index()` expanded to explicit canonical sections:
  - `DISPLAY`
  - `SCREEN`
  - `WINDOW`
  - `INPUT`
  - `APP`
  - `OVERLAY`
  - `MEDIA CONTROL`
- `invoke_discovery_catalog()` expanded to the broad canonical list
- remaining production `CommandSpec.id` literals renamed from `debug.*` / `verify.axText` to canonical ids
- survivor `debug.fixtureObserve` renamed to `fixture.observe`
- stale summary/help strings updated from old ids to canonical ids
- catalog tests updated and new guard tests added for:
  - old `debug.*` / `verify.axText` fail
  - canonical ids resolve
  - no production id starts with `debug.` / `verify.`
  - survivors still resolve
  - survivors excluded from discovery/help
  - help index uses canonical ids only

Important implementation fact re-confirmed during planning and preserved in code:
- discovery is **hand-curated twice**, not namespace-filtered:
  - `invoke_discovery_catalog()` explicit list
  - `render_invoke_help_index()` explicit section lists
- therefore survivor exclusion is list-based, not namespace-based

#### `src/app/mod.rs`
String-keyed runtime consumers were updated to canonical ids:
- `debug.probePermissions` → `app.probePermissions`
- `debug.probeCoordinateReadiness` → `display.probeCoordinateReadiness`
- `debug.activateApp` → `app.activate`
- `debug.observeWindowRegion` → `window.observeRegion`

The earlier 4 C1c fixes remain:
- `display.list`
- `window.list`
- `window.captureAxTree`
- `window.capture`

#### `src/app/tests.rs`
- real-catalog guard array `APP_PROBE_COMMAND_IDS` updated to canonical probe ids

#### `src/scroll_scan/mod.rs`
Updated live string-keyed `InvokeRequest.command_id` values:
- `debug.observeWindowRegion` → `window.observeRegion`
- `debug.scrollWindowRegion` → `window.scrollRegion`

Also updated the test-local fixture catalog entries in `scroll_scan_test_runtime()` to:
- `window.observeRegion`
- `window.scrollRegion`
- `fixture.observe`

#### stale hint / help string cleanup completed
- `src/cli.rs` NOTES block updated from old debug/verify names to canonical ids
- `src/driver/macos/support/typed_capture.rs` hint updated to `window.list`
- `src/driver/macos/control/window_ocr.rs` hint updated to `window.findText`
- `src/driver/macos/control/ax.rs`
  - stale `debug.findWindowText` hint updated
  - stale `debug.clickWindowText` fallback hint updated
  - stale `debug.axClickWindowText` evidence text updated
- `crates/auv-driver-macos/src/support/selector.rs`
  - remaining `debug.listWindows` hints updated to `window.list`
- `src/driver/macos/tests.rs`
  - stale string assertions updated to match `window.list`

## Parallel review status

### First review round
1. `review catalog/discovery logic`
- verdict: `APPROVE`
- no high-confidence semantic bug found
- judged discovery/help/survivor handling coherent

2. `review current C1d diff`
- invalid review surface
- it inspected `main...HEAD` instead of the working-tree diff and therefore missed active uncommitted changes
- main-thread judgment: ignore as low-confidence / wrong diff surface

### Final review rounds after validation
1. `ecc:security-reviewer`
- no high-confidence security or safety regressions found
- no hidden survivor exposure, alias backflow, execution-surface widening, or consent/runtime boundary drift found

2. `ecc:rust-reviewer`
- first pass found 2 real issues in `src/cli.rs`:
  - help text still described stale discovery semantics
  - parser tests still asserted `debug.captureDisplay`
- both issues were fixed in the working tree
- final pass found no material C1d issue
- only noted unrelated existing `cargo clippy -- -D warnings` baseline noise outside this slice

3. `ecc:code-reviewer`
- verdict: `APPROVE`
- judged the final working-tree diff semantically self-consistent across catalog/help/runtime consumers/tests

## Validation status

Validation was run locally to completion in this session.

Passed:
- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `git diff --check`

Recorded test output artifacts:
- `cargo test` pass log: `/Users/liuziheng/.claude/projects/-Users-liuziheng-https-github-com-moeru-ai-auv/d0721cc9-dcc4-480a-bf6a-84bbf47170bb/tool-results/b44cf3vv1.txt`
- earlier successful `cargo test` log: `/Users/liuziheng/.claude/projects/-Users-liuziheng-https-github-com-moeru-ai-auv/d0721cc9-dcc4-480a-bf6a-84bbf47170bb/tool-results/bnlbzigv3.txt`

Not yet run in this session:
- `cargo run -- invoke --help`
- `cargo run -- invoke <renamed-id> --help`
- `cargo run -- invoke debug.typeText` (must fail)
- `cargo run -- invoke verify.axText` (must fail)
- `cargo run -- invoke music.result.play --help` (must be hidden from discovery)
- `auv-cli app probe <bundle-id>`

So the Rust validation block required by `CLAUDE.md` is complete, but the extra smoke commands from the C1d plan remain available as an additional manual confirmation layer.

## Important do-not-touch / confirmed-not-runtime items
These were explicitly investigated and should NOT be treated as unresolved command-id consumers:
- `src/driver/macos/observe.rs`
  - `VERIFY_AX_TEXT_OPERATION_ID`
  - `VERIFY_MUSIC_NOW_PLAYING_OPERATION_ID`
  - recording / `OperationResult.operation_id` labels, not catalog resolves
- `src/candidate_action_decision.rs` and `src/candidate_promotion_recording.rs`
  - `debug.captureAxTree` occurrences there are provenance / freshness labels, not live runtime command dispatch
- `"auv.scan.window_region"` in `src/scroll_scan/mod.rs`
  - RunSpec name, not a command id

## What remains after C1d
C1d itself is done for the approved scope and has since been pushed.

This document should now be read as a historical C1d closure record, not as the
current live execution state.

Still explicitly deferred at C1d close:
- `src/catalog.rs` deletion
- Runtime registry extraction
- any C1e work
- any silent fold into C2

Later progress after this handoff:
- C2a was rejected as a hollow standalone slice.
- C2b became the first real C2 slice and is complete locally via:
  - `e99b032` — `refactor(recording): detach recorded operation staging from runtime internals`
  - `4fd30ba` — `refactor(recording): complete gate for C2b recorded operation detach`
- C2b validation passed locally:
  - `cargo fmt --check`
  - `cargo check`
  - `cargo test recorded_operation -- --nocapture`
  - `cargo test`
  - `git diff --check`
- Fine-grained review passed after fix/re-review loops.
- C2c completed locally and validated by moving inspect/read helpers off `Runtime`
  into explicit read-side entry points in `inspect` / `run_read`.
- C2d completed locally and validated by replacing direct `Runtime` ownership of
  `CommandCatalog` with `RuntimeCommandRegistry`, while preserving invoke behavior.
- C2d validation/review evidence was also checked into Collabi session
  `auv-core-c2d-runtime-registry-detach`.
- The next expected slice is C2e: shrink `Runtime` toward a thinner facade and
  remove remaining dead recording/registry paths.
- Later progress after this handoff also closed the whole C3 lane locally:
  - `C3a` rehomed `steam.library.list.v0` from `fixture.observe` to the honest
    `steam.local` driver.
  - `C3b` made the `auv-steam` bin and the core command share the same library
    entry via `query_local_library_apps(...)`.
  - `C3c` added regression coverage that pins structured evidence and inspect
    shape for the command.
  - C3 closure details live in
    `docs/ai/references/apps/game-observe/2026-06-14-steam-core-closure.md`.

The next slice, if chosen by the owner, must still be approved separately.

## One-line truth of repo state right now
C1d is historical and complete; C2c/C2d are now complete and validated locally, and the next core slice is C2e rather than deferred registry extraction.
