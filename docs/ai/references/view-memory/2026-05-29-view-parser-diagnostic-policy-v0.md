# AUV View Parser Diagnostic Policy v0

Date: 2026-05-29

Status: v0 policy spec. Pins **when** each `ParserDiagnosticKind` fires
and **how** it interacts with the parser's success / failure decision.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
implementing view parser layers or readers that consume parser
diagnostics from stored reconstructions.

## Purpose

`docs/ai/references/view-memory/2026-05-29-view-parser-ir-shapes-v0.md` defines the
`ParserDiagnosticKind` enum with 10 variants but does not pin firing
conditions, severity, or whether each variant stops the parser. Without
this policy:

- Different parser layers (App / View / Region / Item) emit the same
  diagnostic for different conditions.
- Some Codex-written parsers bubble `Err(...)` for situations a future
  parser handles as `Ok(reconstruction)` with a diagnostic.
- `known_limits` and `diagnostics` overlap in messy ways because there is
  no rule for which carries which signal.
- Readers cannot map a diagnostic kind to a fixed severity without
  re-reading every parser.

This document closes those questions for v0.

## Relationship to other specs

```text
view-parser-ir-netease-playlist-example-design.md   what & why
surface-analyze-v0.md                                surface candidates & promotion
view-parser-contract-bridge-v0.md                    must-use existing contracts
view-parser-ir-shapes-v0.md                          concrete IR types
view-parser-diagnostic-policy-v0.md   (this doc)     when each diagnostic fires
```

## Error versus diagnostic

The parser returns `AuvResult<ViewReconstruction>`. v0 distinguishes
three outcome shapes:

| Outcome | Returns | When |
|---|---|---|
| Clean success | `Ok(reconstruction)` with empty `diagnostics` | parser ran end-to-end without surprises |
| Observed failure | `Ok(reconstruction)` with at least one Fatal diagnostic and matching `known_limits` entry | parser observed the failure but cannot proceed (modal blocked, region missing, region collapsed) |
| Infrastructure failure | `Err(...)` | the parser could not run its evidence pipeline (driver capture failed, AX tree capture errored, OCR backend unreachable) |

This mirrors the surface-analyze v0 convention: a verifiable observation
that the world disagrees with our assertion is **not** an `Err`. `Err`
is reserved for "we could not even observe".

Implementations that route every failure through `Err` defeat the
diagnostics layer and break readers that expect structured failure
information in the reconstruction artifact.

## Severity is kind-implied

`ParserDiagnostic` in `view-parser-ir-shapes-v0.md` does **not** carry a
severity field. Severity is fixed per `ParserDiagnosticKind` and lives
here, not on the wire. Readers consult this table to derive severity.

This avoids parser-side ambiguity ("is this a Warn or an Error?") and
keeps the IR struct tight.

## Firing matrix

| Kind | Severity | Stops parser? | Fires when | Required evidence_refs | Required node_id | Required observation_index |
|---|---|---|---|---|---|---|
| `ConflictingEvidence` | Warn | No | Cross-viewport merge rule (2), (3), or (4) from IR shapes spec fails between two candidates | Both candidates' evidence | Both candidates' ids | Both observation indices |
| `IncompleteEvidence` | Warn | No | A candidate has signals from one source but not the corroborating signals expected by its `kind_hint` (e.g. OCR text without bounds, AX node without text) | The partial source | Candidate or empty if pre-merge | The observation the partial source came from |
| `ScrollStuck` | Error | Yes (scroll loop) | A scroll action completed but viewport fingerprint did not change and bounds did not shift | The before/after capture artifacts | The scrollable container | Both observations |
| `RepeatedViewport` | Info | No | The same `ViewportFingerprint` recurs across non-adjacent observations | The capture artifacts producing both observations | The scrollable container if known | All observations sharing the fingerprint |
| `SectionAmbiguous` | Warn | No | An item candidate has evidence consistent with two or more sections and no tiebreaker | Evidence supporting both section assignments | The ambiguous item | The observation where ambiguity surfaced |
| `ItemPartiallyVisible` | Info | No | An item bounds extends beyond viewport bounds on at least one edge | The capture or OCR evidence with the clipped bounds | The partial item | The observation |
| `ModalBlocked` | Fatal | Yes | Modal, popover, system dialog, or permission dialog detected over the target region during parsing | Evidence of the modal (AX detection, capture, OCR signature) | Empty or modal node if one was emitted | The observation that detected the modal |
| `RegionNotFound` | Fatal | Yes | Region detection ran but produced no candidate region matching the `ViewRegion.region_id` target | All region-detection attempt evidence | Empty | The observation pass that attempted detection |
| `RegionResized` | Warn | No | Region bounds detected in observation N+1 differ from observation N beyond a tolerance the implementation declares | The bounds evidence from both observations | The region's root node id | Both observations |
| `RegionCollapsed` | Fatal | Yes | Region detected but bounds are below the minimum width / height the parser declares parseable | The bounds evidence | Empty | The observation pass that detected collapse |

## Per-kind details

### `ConflictingEvidence` (Warn, continues)

Fires only at merge time. Do **not** fire it for ambiguous single-observation
candidates (use `SectionAmbiguous` or `IncompleteEvidence`). The
diagnostic message must name which merge rule failed.

Both candidates remain as separate `ViewNode`s. Implementations must
not silently pick one and continue without a diagnostic.

### `IncompleteEvidence` (Warn, continues)

Fires when expected-by-kind signals are absent. The parser still emits
the candidate / node so reviewers can see what was found, with the gap
recorded.

Do not use this to flag "ideally would have more data". Reserve it for
"the kind contract expects X and X is missing".

### `ScrollStuck` (Error, stops scroll loop)

The scroll loop stops on the first `ScrollStuck`. The parser may still
emit a `ViewReconstruction` from observations collected before the
stuck event; the reconstruction's `ScrollBoundary.bottom` (or `top`,
depending on direction) becomes `Contradicted`.

`ScrollStuck` does not bubble `Err`. Higher-level callers see
`Ok(reconstruction)` with the diagnostic and the affected boundary
state.

### `RepeatedViewport` (Info, continues)

Fires only when the repeated fingerprint is not the immediate previous
viewport. Adjacent repeats are expected at boundaries and are recorded
through `ScrollBoundary.repeated_viewport_fingerprints`, not as
diagnostics. Non-adjacent repeats often signal a scrollable region that
loops or a reset.

### `SectionAmbiguous` (Warn, continues)

Fires once per ambiguous item. The item's `section_hint` becomes
`None`. Do **not** fall back to "most likely section" without
evidence; ambiguity must be visible.

### `ItemPartiallyVisible` (Info, continues)

Fires for each observation where the item is clipped. The candidate's
`label` should record the visible portion only. Cross-viewport merge
may later combine partial views into a non-clipped node; in that case
the merged node carries no `ItemPartiallyVisible` diagnostic for the
clipped observations (the merge resolved them).

### `ModalBlocked` (Fatal, stops)

Fires the moment a modal is detected. The parser must not continue
parsing under the modal. The reconstruction is emitted with whatever
was observed prior to the modal plus this diagnostic; `known_limits`
gains an entry naming the modal kind.

This is the canonical "observed failure" — `Ok(reconstruction)` plus a
Fatal diagnostic, not `Err`.

### `RegionNotFound` (Fatal, stops)

Fires when region detection completes without a matching region. The
parser emits an empty reconstruction with this diagnostic. The
diagnostic's evidence_refs must include each detection attempt (OCR
anchor search, AX search, geometry probe) so reviewers can see what
was tried.

### `RegionResized` (Warn, continues)

Fires when bounds drift between observations. Implementations declare
their resize tolerance and document it in `known_limits`. Below
tolerance: no diagnostic. Above tolerance: this diagnostic + continue.

Reuse of stale bounds across the resize is forbidden; subsequent
observations must reflect the new region bounds.

### `RegionCollapsed` (Fatal, stops)

Fires when the detected region is technically present but unusable
(e.g. sidebar collapsed to a strip). Distinct from `RegionNotFound`:
this means "I see it, but it is not parseable". The diagnostic's
message must include the observed width / height and the parser's
declared minimum.

## Evidence ref requirements

Every diagnostic must attach at least one `EvidenceRef` unless its row
above marks evidence_refs as not required. The implementation must not
emit a diagnostic that says "X happened" without naming the artifact
the reader can open to verify.

Diagnostics that span multiple observations (`ConflictingEvidence`,
`ScrollStuck`, `RepeatedViewport`, `RegionResized`) must list refs from
every involved observation in order of occurrence.

## Aggregation rules

When the same kind fires multiple times against the same target during
one parse run:

- `ConflictingEvidence`, `SectionAmbiguous`, `IncompleteEvidence` —
  emit one diagnostic per (kind, target). Do not collapse across
  different targets.
- `ItemPartiallyVisible`, `RepeatedViewport`, `RegionResized` —
  emit one diagnostic per occurrence (per observation pair); do not
  collapse into a single "happened N times" entry.
- `ScrollStuck`, `ModalBlocked`, `RegionNotFound`, `RegionCollapsed` —
  fire at most once per parse run; the parser stops on the first.

## known_limits coupling

When a Fatal diagnostic fires, the parser **must** add a parallel
`known_limits` entry on `ViewReconstruction` that names the failure in
human-readable form. Example:

```text
diagnostic: ModalBlocked
known_limits entry: "permission dialog over sidebar region; parser aborted before scroll loop"
```

This duplication is intentional: machine-readable diagnostic for the
viewer, human-readable known_limit for the CLI / reviewer summary.
Non-fatal diagnostics do not require a `known_limits` entry.

## v0 done criteria

The diagnostic policy is v0-complete when:

1. All 10 `ParserDiagnosticKind` variants are emitted by view parser
   code only under the conditions in the firing matrix.
2. No view parser layer returns `Err(...)` for a condition that has a
   matching Fatal diagnostic. Infrastructure failures only.
3. Severity is derived from kind by readers, not stored on the wire.
4. Every emitted diagnostic carries the evidence_refs and ids the
   matrix requires.
5. Fatal diagnostics are matched by a `known_limits` entry on the
   enclosing reconstruction.
6. Unit tests cover at least one positive and one negative case per
   kind (it fires when it should; it does not fire otherwise).
7. The NetEase example exercises at least `ModalBlocked`,
   `RegionNotFound`, `RegionCollapsed`, `ScrollStuck`,
   `RepeatedViewport`, and `ItemPartiallyVisible` paths.

## Forbidden in v0

- Storing severity in `ParserDiagnostic` on the wire. Use this table.
- Emitting a Fatal diagnostic without setting at least one
  `known_limits` entry on the reconstruction.
- Returning `Err(...)` for `ModalBlocked`, `RegionNotFound`, or
  `RegionCollapsed`. Use the Ok-plus-Fatal-diagnostic pattern.
- Inventing new `ParserDiagnosticKind` variants without a matching
  revision of this document and `view-parser-ir-shapes-v0.md`.
- Collapsing per-target diagnostics into "N items had this issue"
  summary entries.

## Non-goals for this spec

Intentionally deferred:

- Diagnostic localization or rendering rules for the viewer.
- Cross-run aggregation of diagnostics (i.e. "this region has
  collapsed 5 times this week").
- Auto-recovery strategies for `ScrollStuck` / `RegionResized` —
  policy here states the diagnostic; recovery is an implementation
  slice.
- A `ParserSeverity` enum or field. Severity stays implicit per the
  table.
- Diagnostic budget / rate limiting per parse run.

## How to use this spec

When writing or reviewing parser code:

- Look up the diagnostic kind in the firing matrix before adding a new
  emission site.
- If a real condition has no matching kind, file a gap in this doc
  before inventing one.
- Default to `Ok(reconstruction)` plus diagnostic. `Err(...)` is
  reserved for "the pipeline itself failed".
- When unsure whether to fire, fire — diagnostics are auditable;
  silent omissions are not.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
