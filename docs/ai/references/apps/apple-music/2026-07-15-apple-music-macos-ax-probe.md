# Apple Music macOS AX surface probe

**Status:** bounded discovery only (no click, no play, no candidate algorithm)

## Scope

This slice adds `auv-apple-music probe-macos`, a bounded AX discovery probe for
Music.app on macOS:

1. Activate `com.apple.Music` via `ApplicationControl::activate_bundle_id`
   (no `CGWindowID` / WindowServer discovery required, same seam as PR #106).
2. Capture the AX tree via `AccessibilityApi::capture_app_tree`.
3. Filter observed nodes into `search_field_candidates`: nodes matching
   `AXTextField`/`AXSearchField` role/subrole, or a search-related
   placeholder/title.
4. Optionally persist the raw AX snapshot as JSON to `--artifact-dir`.

## Explicitly not done in this slice

- No search query is submitted. The probe only observes Music.app's
  default/landing surface.
- No result-row candidate list. An earlier draft of this probe classified
  `AXRow`/`AXStaticText` nodes with non-empty text as "result rows" without
  ever submitting a search — on the landing surface that heuristic would
  misclassify sidebar labels, buttons, and recommendation copy as search
  results. Removed until a query-submission slice can capture a real
  post-search AX tree to validate against.
- No click/press on any discovered node.
- No playback action (play, pause, transport).
- No `OperationResult` / `StepOutcome` contract (this is a probe command, not
  a product operation).
- No support-matrix row or evidence-level claim.
- No general candidate-selection algorithm. `is_search_field_candidate` is a
  heuristic role/text matcher for probe output only; it is not the
  deterministic candidate contract that `app.apple_music.search_and_play`
  will need.

TODO(apple-music-search-and-play): candidate selection, action delivery, and
now-playing verification are deferred until this probe's real AX shape from
Music.app is captured on real macOS hardware. Do not implement
`search_and_play` from this doc alone — it describes the shape AUV has not
yet observed. Unlocked once an owner reviews a live-captured `--artifact-dir`
snapshot and approves the next slice (search query submission, deterministic
result selection, or now-playing verification) as a narrow follow-up.

## Manual command

```sh
cargo run -p auv-apple-music --bin auv-apple-music -- \
  probe-macos --artifact-dir /tmp/auv-music-probe --json
```

## Live result (2026-07-15)

Run on real macOS hardware (Darwin 27.0, macOS 27.0 build 26A5368g, arm64):

```json
{
  "command": "probe-macos",
  "bundle_id": "com.apple.Music",
  "activated": true,
  "ax_snapshot_captured": true,
  "node_count": 77,
  "search_field_candidates": [],
  "artifact": "/tmp/auv-music-probe/music-ax-probe-1784176575772.json",
  "diagnostics": ["no search field candidates found"]
}
```

`search_field_candidates` is empty. The probe correctly reported failure
rather than fabricating a match — the diagnostics path worked as designed.

### Why the search field was not found

Inspecting the full 77-node snapshot:

- The left sidebar (`0.0.0.0.*`) contains navigation rows Search / Home /
  Radio / Library / Artists / Albums / Songs / Store / Playlists etc. Node
  `0.0.0.0.0.0.1` has `value="Search"`, but this is the sidebar's **"Search"
  navigation item** (an `AXStaticText`), not a text input.
- `0.1` is an `AXToolbar` at the window top (`980x52`, the plausible location
  for a search field) — but it was captured with **zero children**.

No node in the tree is `AXTextField`/`AXSearchField`, and the toolbar subtree
where a search input would likely live was not expanded by the capture.
Possible causes (unverified, listed as hypotheses only):

1. The toolbar's search affordance is a collapsed magnifying-glass button
   that only materializes an `AXTextField` after being clicked/expanded —
   common in recent macOS toolbar search UIs.
2. The AX tree capture's traversal (depth/children limits, or how it walks
   `AXToolbar` specifically) did not descend into the toolbar subtree for
   another reason.

This is a **more upstream blocker than result-row classification** — before
result rows can even be evaluated, the search field itself is not reachable
in the tree this probe captures on the landing surface. No fix was attempted
in this slice; this finding is recorded for the next slice to investigate
(e.g., a probe that first activates/expands the toolbar search affordance,
or one that captures with different traversal parameters, before re-checking
for `AXTextField` nodes).

## Toolbar reachability diagnostic

The follow-up diagnostic resolves each captured `AXToolbar` path against the
current `AXChildren` tree, verifies the observed role, and independently reads
`AXChildren`, `AXVisibleChildren`, `AXContents`, and
`AXChildrenInNavigationOrder`. This is evidence gathering only; it does not
change capture traversal.

`AxNodeInspection` remains on the explicitly temporary, doc-hidden
`native::ax_tree` compatibility surface and is not exposed through the stable
`AccessibilityApi` or crate-root re-exports. Rust requires the native type and
function to remain publicly callable while Apple Music is a separate crate;
that visibility is an implementation constraint, not API graduation. The four
child-attribute counts encode this probe's current hypothesis and have no
second independent consumer, so they are not a stable core driver capability.
The Apple Music crate maps successful reads into its app-local
`ToolbarInspection` output. Per-toolbar failures remain partial diagnostic
outcomes: the probe keeps successful inspections and writes each failed path,
role, and driver error into `diagnostics` instead of silently dropping it.

## PR #111 quality-slice record (2026-07-16)

- Classification: approved feature hardening across `auv-driver-macos`
  (`core-maintained`) and `auv-apple-music` (`reference-maintained`).
- Checked: the PR diff and CI annotations, all three Swift AX path walkers,
  Rust exposure through `types`, `AccessibilityApi`, and the crate root, probe
  serialization/CLI output, the support matrix, and this reference note.
- Rejected: silently dropping per-toolbar errors, and stabilizing the
  hypothesis-specific driver type.
- Selected: one Swift path resolver shared by action/focus/inspection;
  temporary native diagnostics mapped to an app-local result rather than
  adding them to stable `AccessibilityApi` or crate-root re-exports;
  per-item diagnostic errors.
- Scope decision (2026-07-18): the `activated` / `ax_snapshot_captured`
  success booleans on `ProbeResult` (introduced by #108) are kept, not deleted.
  Removing them would change the probe JSON schema — a compatibility change
  that does not serve this PR's AX-diagnostic goal — so it is held out of this
  slice. Revisit deleting them, or replacing them with a real partial-result
  shape, in a dedicated schema PR if a consumer ever depends on the probe
  output.
- Follow-up (2026-07-17): the inspection FFI (`NativeAxNodeInspectionResponse`
  and `DecodedAxNodeInspectionResponse`) carried `subrole`/`title` that Swift
  read and marshalled but the decode step dropped before building
  `AxNodeInspection`. Those two fields were removed from the Swift constructor,
  the bridge struct, and the decode struct so the inspection surface carries
  only the diagnostic-relevant role, child counts, actions, and attributes.
  Bridge regenerated; SwiftPM build, `cargo test`, clippy, and format re-run
  clean.
- Qodana root cause: `ProbeResult` referenced `AxNodeInspection`, but that type
  was imported only under `cfg(target_os = "macos")`. The app-local,
  target-independent output type removes the unresolved path.
- Validation path: generated Swift bridge files, SwiftPM build, RustRover
  diagnostics/build, focused crate tests, workspace check, clippy, format, and
  diff checks. Full workspace tests were attempted but the unrelated
  `auv-driver-linux::ocr::rejects_buffer_with_mismatched_length` test fails on
  macOS because it receives `Unsupported` instead of `InvalidImage`. The
  SwiftPM manifest's bridging-header path was corrected so the required package
  build runs from `native/swift` as documented.
- Core API decision: no stable core public API was added, no provisional term
  was added, and no new error type crossed a Rust crate boundary.
