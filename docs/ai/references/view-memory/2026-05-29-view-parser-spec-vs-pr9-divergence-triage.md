# View Parser v0 Spec Corpus vs PR #9 Implementation: Divergence Triage

Date: 2026-05-29

Status: triage note. **Not a spec.** This document lists the
observed differences between the 17-doc v0 spec corpus and what
PR #9 (`examples/netease_playlist_ls.rs`, 2663 lines) actually
shipped, and proposes a concrete test or check that would tell the
team which approach is healthier for AUV.

The motivation: PR #9 was developed in parallel with the spec
corpus, and the two went meaningfully different directions on
~12 decisions. Both authors should not assume their own choice is
correct. This note lays out the decisions so they can be resolved
by evidence rather than by author.

## How to read this

For each divergence:

- **Spec position** — what `docs/ai/references/2026-05-29-view-parser-*`
  says.
- **PR #9 position** — what `examples/netease_playlist_ls.rs` does.
- **What is lost on each side** — be honest about the trade-off.
- **Resolver** — the smallest test, measurement, or example that
  would tell the team which is healthier.

The team should pick a side per row. There is no obligation to
take one side wholesale.

---

## 1. Tree shape: flat node store vs nested children

- **Spec position** (ir-shapes-v0): `ViewReconstruction.nodes:
  BTreeMap<ViewNodeId, ViewNode>`; tree edges by ID. JSON readers
  do a single dictionary lookup; no recursive nesting.
- **PR #9 position**: `ViewNodeRecord` carries `children:
  Vec<ViewNodeRecord>` directly. Recursive structure.

**Trade-off.** Recursive is simpler for one writer; flat is safer
for cross-document references (anchors, landmarks, projection
records) because deep nodes are reachable by ID without tree walks.

**Resolver.** Write `playlist get <anchor_id>`. If the recursive
tree forces a depth-first walk to find the node, that is a real
cost paid every read. If the flat store makes anchor lookup O(1),
that is the practical win. Measure both on a 100-item NetEase
sidebar.

## 2. ArtifactRef reuse vs absent

- **Spec position** (bridge-v0): every evidence pointer is a
  `contract::ArtifactRef { run_id, span_id, artifact_id,
  captured_event_id }`. No `ArtifactRefLike`.
- **PR #9 position**: `evidence_ids: Vec<String>` on nodes, anchors,
  landmarks. String IDs only; no run/span/event traceability.

**Trade-off.** PR #9 is simpler for a single-file example. The
spec position is heavier but the only path that makes view parser
artifacts joinable with the rest of the run trace (capture, OCR
results, AX dumps) without parsing names.

**Resolver.** Open the inspect viewer on a parse run. Can a reviewer
click a `ViewNode` and reach the OCR result that backed it? With
string IDs, no. With ArtifactRef, yes (per inspect-viewer-v0).
This is the substantive difference; if you want this in the viewer,
the spec wins.

## 3. Coordinate space tracking vs ambient f64

- **Spec position** (ir-shapes-v0): `ViewBounds { origin_space:
  CoordinateSpace, ... }` with explicit `WindowLocal / RegionLocal /
  ViewportLocal / DisplayPhysical`.
- **PR #9 position**: `ViewBounds { x: f64, y: f64, ... }`. No
  coordinate space.

**Trade-off.** PR #9 assumes everything is in the same implicit
coord space. Probably true for a single-window sidebar parse.
Breaks the moment you compose two regions or two windows.

**Resolver.** Add a second region to the same scope (e.g. NetEase
playlist sidebar + main play view). If both regions need to
correlate bounds, the implicit space fails silently; the typed
space fails at compile.

## 4. Boundary axes: top/bottom vs top/bottom/left/right

- **Spec position** (ir-shapes-v0): `ScrollBoundary { top, bottom,
  repeated_viewport_fingerprints }`. Vertical only.
- **PR #9 position**: `ScrollBoundarySummary { top, bottom, left,
  right }`. All four sides.

**Trade-off.** PR #9 covers horizontal scroll. My spec implicitly
assumed vertical because NetEase sidebar is vertical, but that
choice excludes horizontal scrollables.

**Resolver.** This one is straightforward: **PR #9 wins**. The spec
should adopt the 4-axis shape. Cost is one extra enum field per
boundary; benefit is horizontal scrollable support without a v1
bump.

## 5. Diagnostic kind: typed enum (10 variants) vs string code

- **Spec position** (diagnostic-policy-v0): `ParserDiagnosticKind`
  is a closed enum of 10 variants; firing matrix per kind;
  kind-implied severity.
- **PR #9 position**: `ParserDiagnostic { code: String, message,
  node_id }`. Free-form string code.

**Trade-off.** Strings let any parser invent a code without spec
revision. The enum forces every diagnostic into one of the 10
known categories, which is the whole point of having policy.

**Resolver.** Grep PR #9's actual diagnostic emissions (search for
`ParserDiagnostic {`). If every emitted `code` value fits cleanly
into one of the 10 enum variants, the spec wins on discipline
without losing flexibility. If real cases need codes that none of
the 10 cover, the enum is too tight.

## 6. Diagnostic evidence: typed refs vs absent

- **Spec position** (diagnostic-policy-v0): every diagnostic
  carries `evidence_refs: Vec<EvidenceRef>` so a reviewer can open
  the artifact that triggered it.
- **PR #9 position**: no evidence refs on `ParserDiagnostic`.
  Reader sees code + message + node_id and must search.

**Trade-off.** Same trade-off as #2 but for diagnostics.

**Resolver.** Same as #2: this is the viewer-ergonomics question.
If you want to click a `RegionNotFound` and see the failed AX /
OCR / geometry detection artifacts, the spec wins.

## 7. Confidence: rich vs enum-only

- **Spec position** (ir-shapes-v0): `Confidence { level:
  ConfidenceLevel, provider_scores: BTreeMap<String, f64> }`.
  Carries per-provider raw scores.
- **PR #9 position**: `Confidence` is an enum only (no provider
  scores).

**Trade-off.** PR #9 throws away the per-provider scores. The spec
keeps them so reviewers can see which provider was confident and
which dissented. That data exists during item parsing (OCR
returns a score); the only question is whether to persist it.

**Resolver.** Look at a real run's diagnostics where confidence is
`Likely` or `Unknown`. Can a reviewer tell whether OCR or icon
disagreed? With PR #9, no. With the spec, yes. If you do not need
that distinction for tuning the `REVIEW(confidence-mapping)`
threshold from item-parsing-v0, PR #9 is fine.

## 8. Schema versioning: required field vs none

- **Spec position** (ir-shapes-v0): `schema_version: "view-ir-v0"`
  required on every top-level artifact; readers reject other
  versions.
- **PR #9 position**: no `schema_version` field.

**Trade-off.** Without `schema_version`, the first format change
silently breaks every stored artifact. PR #9 is fine for an
example but not for stored artifacts that survive across releases.

**Resolver.** Will any stored artifact be read across a code
upgrade? If artifacts are throwaway per-run, PR #9 is fine. If
the inspect viewer or `playlist get` will read older runs, the
spec wins.

## 9. ID derivation: content-hash deterministic vs free String

- **Spec position** (ir-shapes-v0): `ViewNodeId` derived from
  `hash(region_id, section_hint, normalized_label)` (Items),
  similar rules for Section / Container / etc. Same node → same
  ID across runs.
- **PR #9 position**: `id: String`. No derivation rule.

**Trade-off.** Without derivation, the same node has a different
ID across runs, so memory-based reacquisition cannot use IDs.
This is why my spec spent the effort: it enables `playlist get
<anchor_id>` to be a stable contract.

**Resolver.** Run a parse twice on an unchanged sidebar. Compare
the `id` values of "Liked Songs" across the two runs. If they
differ, you cannot save an anchor that survives a re-scan; that
forecloses ViewMemory + reacquisition.

## 10. ViewMemory + reacquisition: spec'd vs absent

- **Spec position** (view-memory-v0, anchor-reacquisition-v0):
  full persistence shape, freshness rules, eviction, 6-stage
  cascade with bounded budgets, span namespace.
- **PR #9 position**: not implemented. Single-shot `playlist ls`.

**Trade-off.** PR #9 ships sooner but `playlist get <anchor>`
requires a full re-scan each time. The spec is heavier but covers
the follow-up command surface.

**Resolver.** Is `playlist get` actually on the roadmap soon? If
yes, the spec's shape becomes load-bearing the moment that
command lands. If `playlist ls` is the only command for the
foreseeable future, the spec's memory layer is premature.

## 11. Inspect viewer integration: spec'd vs absent

- **Spec position** (inspect-viewer-v0): `Runtime::list_view_*` ×
  4, HTTP envelope additions, viewer HTML tab, color / severity
  mapping.
- **PR #9 position**: not integrated. CLI-only.

**Trade-off.** Same as #10: PR #9 ships sooner; the spec covers
the read-side surface.

**Resolver.** Open a recent parse run in the inspect viewer.
Without integration, what do you see? Span list only. Is that
enough? If reviewers are happy clicking spans and reading raw
artifact JSON, integration is premature.

## 12. Crate placement: two new crates vs single-file example

- **Spec position** (example-placement-v0): `crates/auv-view` +
  `crates/auv-example-netease-playlist`. Domain logic in the
  example crate, generic IR in `auv-view`.
- **PR #9 position**: `examples/netease_playlist_ls.rs` only. No
  generic IR crate; types are local to the example file.

**Trade-off.** PR #9 is fine for one example. The spec's two-crate
layout is the only path that supports a second example sharing
the IR without copy-paste.

**Resolver.** Is there a planned second view parser example (e.g.
Spotify, Apple Music, a non-music app)? If yes, the spec's
layout is the path of least pain. If no, PR #9 is fine until
the second example actually exists.

---

## Summary table

| # | Topic | Suggested resolution heuristic |
|---|---|---|
| 1 | Tree shape | Test against `playlist get` lookup |
| 2 | ArtifactRef reuse | PR #9 unless you want viewer click-through |
| 3 | Coord space typing | PR #9 unless you compose two regions |
| 4 | Boundary axes | **PR #9 wins** (4 sides > 2) |
| 5 | Diagnostic kind enum | Grep emissions; if they fit 10 kinds, spec wins |
| 6 | Diagnostic evidence | Same as #2 |
| 7 | Confidence richness | Spec wins if you tune confidence thresholds with data |
| 8 | Schema version | PR #9 unless artifacts survive a release |
| 9 | ID derivation | Test 2x parse identity; spec wins if memory matters |
| 10 | ViewMemory + reacquire | Spec wins iff `playlist get` is planned |
| 11 | Inspect viewer integration | Spec wins iff viewer is the planned UI |
| 12 | Crate placement | PR #9 fine until a second example exists |

## Practical next step

This note is not asking the team to decide all 12 at once. The
honest minimum:

1. Read this list together.
2. Mark each row as `pick PR #9` / `pick spec` / `keep both` /
   `defer`.
3. For each `pick PR #9` row, the corresponding section in the
   spec corpus gets a `Superseded by PR #9, see triage` note.
4. For each `pick spec` row, PR #9 gets a follow-up issue.
5. For each `keep both` row, the spec and PR #9 both exist and
   readers know that the answer is environment-dependent (e.g.
   `playlist ls` vs `playlist get` use different shapes).
6. For each `defer` row, the spec stays but the PR #9 code is
   not required to adopt it until a triggering event lands.

The triage is the v0 reconciliation. After it, the corpus is
either smaller (specs retired) or annotated (specs marked
superseded), and PR #9 is either accepted as-is or has
follow-up gaps named.

This document is part of the convergence phase. It is itself
transient — once the team has triaged the rows, this note's job
is done and it can be retired.
