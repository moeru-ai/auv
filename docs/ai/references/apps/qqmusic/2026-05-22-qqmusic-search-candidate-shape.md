# QQ音乐 Search — Candidate Shape, Derived from Existing Probe Evidence

Date: 2026-05-22

Status: contract draft, evidence-grounded

## Why this doc exists

The next step on the AUV mainline (per
`2026-05-22-phase-3-mainline-acceptance.md`) is moving from primitive
commands to a first-class operation contract:

- `OperationResult` — unified shape every getter/action returns
- `Candidate` — first-class addressable thing an agent can `select(id)`
- `VerificationResult` — unified shape every verifier returns

The user-stated risk: **drafting these types abstractly and then
trying to make QQ音乐 fit them.** That's how contract docs become
fiction.

So this doc starts from the other direction: read what we **already
know** about the QQ音乐 search surface from existing probe artifacts,
then write down what the `Candidate` type must be to honestly
represent a QQ音乐 search-result row. The Rust types in the next
commit will be constrained by what's recorded here.

## Sources of truth used

This doc does not run a fresh probe. It uses:

- `docs/ai/references/apps/qqmusic/2026-05-15-qqmusic-macos-capability-probe.md` —
  Phase 1 capability probe (315 lines, comprehensive)
- `docs/ai/references/evidence/2026-05-14-qqmusic-search-ocr-anchor/`
  — actual probe artifacts including a 3024x1964 PNG of the `aa`
  search result page and its OCR contract `.txt`
- `recipes/macos/qqmusic/play-visible-anchor.v0.json` (validated)
- `recipes/macos/qqmusic/play-visible-row.v0.json` (validated, ASCII
  cases only) + `.v1.json` (Phase 3 #6 smart-press exemption)
- `docs/ai/references/2026-05-21-phase-3-mainline-audit.md` —
  confirms `play_visible_row.v0` still validates as of 2026-05-22

If during implementation the existing evidence turns out to disagree
with current app state, the contract draft falls back to a fresh
hands-off probe. The implementation work itself is the trigger for
that probe, not preemptive paranoia.

## What QQ音乐's search surface actually looks like

From `2026-05-15-qqmusic-macos-capability-probe.md`:

| Layer | What's exposed | What's not |
|---|---|---|
| AX search shell | `文本框 搜索` text field, `面板 搜索` container, `按钮 关闭` close button | settable `AXValue` on the search input |
| AX result list | nothing reliable | row count, row identity, per-row press action |
| Visible content | OCR-readable row titles (cyan-anchor band on the aa page contains "Cure For Me", "Soft Universe", "AA (Alone Again)", "I Drink The Light", "Lovely Place", "Live At Sunset") | stable AX subtree |
| Activation | row double-click via pointer (or single-click via OCR anchor → click_screen_text) reliably starts playback | keyboard navigation through `Tab`/`Down` |
| Bottom player | now-playing title readable via AX **after** activation | live update guarantees during transition |

The structural conclusion that drives the contract:

> **A QQ音乐 search-result row is fundamentally an OCR observation
> tied to a specific screenshot, not an AX entity that survives
> across captures.** Any `Candidate` representing such a row must
> carry its evidence (the screenshot) and must die when the surface
> changes.

## Candidate field shape, derived

What a single QQ音乐 search-result Candidate must encode, with each
field justified by the evidence source above:

| Field | Type sketch | Why it must be there | Evidence source |
|---|---|---|---|
| `candidate_local_id` | string stable within one `OperationResult` only | row index within this getter's output | core contract requirement |
| `kind` | `SearchResultRow` (enum variant) | distinguishes from `SearchInputField`, `PlayerControl`, etc. — same getter could return different kinds | probe shows several AX/OCR layers exist |
| `label` | human-readable string | for logs / viewer rendering / model prompts | `"Cure For Me"` is the OCR row text from the validated case |
| `target_spec.grounding` | `ocr-anchor` (not `ax-node`) | AX rows are not stable on QQ音乐 results | probe section "AX Tree Findings" + "Result Selection Phase" |
| `target_spec.anchor_text` | the OCR string that identifies the row | this is what `click_window_text` consumes downstream | `recipes/macos/qqmusic/play-visible-anchor.v0.json` step `resolve-ocr-anchor` |
| `target_spec.region_hint` | optional `{left,top,right,bottom}` ratios | OCR needs region constraint to disambiguate row text from chrome | recipe `selection_region_*_ratio` defaults |
| `evidence.artifact_ref` | `ArtifactRef` (see below) | proves the candidate was real at capture time + lets viewer overlay-render it | every existing recipe captures evidence; this just makes the back-reference explicit |
| `evidence.observation` | the OCR match shape (text, bounds, confidence, match_index) | what the candidate physically was | already produced by `debug.findWindowText` / `debug.findWindowRows` |
| `liveness.preconditions` | structured (see below) | candidates die; we must declare how | NEW contract field — no recipe captures this today |
| `liveness.ttl_hint_ms` | optional u64 | advisory upper bound, NOT authoritative | NEW |
| `control.requires_app_frontmost` | bool | action-time activation requirement, separate from liveness | NEW; previously conflated with liveness |
| `control.requires_window_focus` | bool | does the action require this specific window to be focused | NEW |

### `candidate_ref` — the cross-operation handle

`candidate_local_id` is **only** stable within the single
`OperationResult` that produced it. Two separate
`music.search.results` calls can both emit a row labeled `"row#1"`
and they will not be the same candidate.

An action that consumes a candidate (e.g. `music.result.play`) MUST
receive a full `candidate_ref`, not just `candidate_local_id`:

```
candidate_ref = {
  source_run_id: RunId,
  source_span_id: SpanId,
  source_operation_id: String,      // e.g. "music.search.results"
  source_artifact_id: ArtifactId,   // the typed Candidate-set artifact
  candidate_local_id: String,
}
```

The action looks the candidate up by reading the source artifact
back from the store. This makes the candidate's evidence chain
immutable and unambiguous: the action cannot accidentally act on
"a candidate that looks similar but came from somewhere else".

### `evidence.artifact_ref` — what an artifact ref actually is

`ArtifactRecordV1Alpha1` (`src/trace.rs:162`) carries
`artifact_id` + `span_id` + optional `event_id` + `role` + `path`
— but **no timestamp**. The timestamp lives on the
`artifact.captured` event that announces this artifact, or on the
parent span's `started_at_millis` / `finished_at_millis`.

So `ArtifactRef` is:

```
ArtifactRef = {
  run_id: RunId,
  artifact_id: ArtifactId,
  span_id: SpanId,                  // mirrors ArtifactRecord.span_id
  captured_event_id: Option<EventId>, // the artifact.captured event,
                                      // when present (it usually is)
}
```

The captured timestamp is then derived as:

```
captured_at_millis =
  if captured_event_id is Some:
    EventRecord(captured_event_id).timestamp_millis
  else:
    SpanRecord(span_id).finished_at_millis
      .or(SpanRecord(span_id).started_at_millis)
```

This keeps the contract honest about where the timestamp comes from
and avoids pretending artifacts carry their own time.

### `liveness.preconditions` — what kills a candidate

This is the field the user called out as missing from the original
proposal. Without it, candidate IDs become "try and pray" handles.

For QQ音乐 search rows, the preconditions that need to still hold
for a candidate to be valid (excluding activation, which is below):

```
liveness.preconditions = {
  window_ref: {
    app_bundle_id: String,
    window_title_substring: Option<String>,
    window_number: Option<i64>,
  },
  anchor_recheck: {
    text: <candidate.target_spec.anchor_text>,
    region_hint: <candidate.target_spec.region_hint>,
    expected_min_confidence: <number>,
    max_pixel_distance: <number>,    // how far the anchor is
                                     // allowed to have moved
  },
}
```

Action handlers consuming a `candidate_ref` MUST re-evaluate these
before acting. If `window_ref` no longer resolves to a real window,
or `anchor_recheck` finds no match (or a match outside the
distance budget), the action returns an `OperationResult` with
`failure_layer: candidate_expired` and an `invalidation_evidence`
artifact (re-capture + diff). It does NOT guess and click anyway.

### `control.requires_*` — action-time activation, NOT liveness

Whether QQ音乐 needs to be the frontmost app at the moment of click
is an **activation requirement of the action**, not a property of
whether the candidate is still real. A candidate captured while
QQ音乐 was foreground does not die the moment the user Cmd-Tabs
away — the row still exists in the window's content, the action
just needs to re-foreground before pressing.

Splitting these into `liveness.preconditions` (does the candidate
still exist) and `control.requires_*` (what does the action need
to happen) prevents the wrong failure mode being reported. A
`control_failed` (couldn't refocus) is materially different from
`candidate_expired` (the row is gone), and downstream agents
should retry differently.

### What's deliberately NOT in this version

Per the user-direction critique, the first version of the contract
intentionally **does not** include:

- `confidence` (numeric 0..1 on the Candidate itself). Reason: we
  don't have repeatability data. Adding a confidence float makes
  it look like there's a real distribution behind it. Until N
  hands-off probes produce empirical variance per anchor, the
  honest answer is "we have evidence or we don't" — captured by
  `evidence` + `liveness.invalidation_preconditions`.
- `valid_until` as a hard wall-clock timestamp. Time-based expiry
  is hopeful; UI state can change in 50ms or be stable for an
  hour. `ttl_hint_ms` is included as an *advisory* only.

These come back when we have measurements that justify them.

## OperationResult shape, derived

A unified shape every getter/action returns. The minimum that fits
both `music.search.results` (getter) and `music.result.play` (action):

| Field | Type sketch | Notes |
|---|---|---|
| `run_id` | string | already exists in `InvokeResult` |
| `status` | `Completed | Failed` | already exists |
| `operation_id` | string | `music.search.results` / `music.result.play` / etc. |
| `evidence_artifacts` | `Vec<ArtifactRef>` | typed back-references to artifacts produced this op |
| `output` | typed variant: `Candidates(Vec<Candidate>)` for getters, `Verification(VerificationResult)` for actions, `Acknowledged` for fire-and-forget | what the agent actually consumes |
| `freshness_basis` | structured: which capture this OperationResult was derived from | so `output.candidates[i].liveness` can reference back |
| `known_limits` | `Vec<string>` | per-result honest disclaimers, e.g. "this query path was validated only for ASCII anchors" |

Fields **not** in v1:
- `confidence` — same reason as above
- `cost` / `cache` / `dedup_key` — premature optimization without
  real consumer pain

## VerificationResult shape, derived

A unified shape every verifier returns. For QQ音乐 the verifier of
interest is "did the now-playing title become what was requested":

| Field | Type sketch | Notes |
|---|---|---|
| `executed` | bool | did we attempt the action |
| `state_changed` | bool | did the underlying UI/system state observably change |
| `semantic_matched` | `Option<bool>` | did the change match the requested intent. `None` when there's no semantic to match (fire-and-forget action) |
| `failure_layer` | `Option<FailureLayer>` | layered enum, see below |
| `evidence` | `Vec<ArtifactRef>` | verifier's own evidence (e.g. post-action screenshot + AX now-playing capture) |
| `observed_label` | `Option<String>` | what we actually observed (e.g. `"天空仍灿烂"` when expecting `"晴天"` — directly maps to the Phase 1 unresolved Chinese boundary) |

### `FailureLayer` — distinguish where the chain broke

The original draft collapsed too many failure modes into too few
variants. A debug trace that lumps "couldn't find the target",
"clicked wrong", and "clicked right but state didn't change" into
the same bucket is useless. The v0 enum:

| Variant | Meaning | Where it fires |
|---|---|---|
| `grounding_failed` | the getter couldn't produce a candidate at all (zero OCR matches, AX subtree empty, etc.) | inside the getter, before any action |
| `candidate_expired` | the action received a `candidate_ref` but its `liveness.preconditions` no longer hold | inside the action, before any user-visible side effect |
| `control_failed` | a `control.requires_*` precondition could not be satisfied (couldn't bring app forward, couldn't focus the window) | inside the action, before the click/press |
| `verification_unreliable` | the action ran but the verifier itself could not observe a clean post-state (e.g. OCR timed out, AX returned `noValue`) | inside the verifier, after the action |
| `state_changed_no_match` | action ran, state observably changed, but the new state isn't the intended one (Phase 1 Chinese boundary case: `"晴天"` requested, `"天空仍灿烂"` observed) | inside the verifier, after a clean observation |
| `semantic_mismatch` | reserved for when the action's pre-declared semantic intent doesn't match what the action actually did (e.g. recipe says "play song X" but actually clicked download) — catches contract drift between recipe declaration and runtime behaviour | cross-cuts; typically reported by the runtime, not by individual drivers |

`failure_layer` is the field that lets the next agent decide
whether to retry with a new candidate (`candidate_expired`), retry
after re-focusing (`control_failed`), back off the recipe entirely
(`verification_unreliable`), or accept a documented
state-change-without-semantic-match
(`state_changed_no_match` → the Phase 1 Chinese boundary case).

## The narrow end-to-end loop this contract has to serve

The contract's first test is a single end-to-end music loop:

```
1. getter:  music.search.results(query="aa")
              -> OperationResult { output: Candidates([
                   Candidate { id: "row#1", label: "Cure For Me", ... },
                   Candidate { id: "row#2", label: "Soft Universe", ... },
                   ...
                 ])}

2. action:  music.result.play(candidate_ref={
                     source_run_id: <getter run>,
                     source_span_id: <getter span>,
                     source_operation_id: "music.search.results",
                     source_artifact_id: <candidate-set artifact>,
                     candidate_local_id: "row#1"
                   })
              -> OperationResult { output: Verification(
                   VerificationResult {
                     executed: true,
                     state_changed: true,
                     semantic_matched: Some(true),
                     observed_label: Some("Cure For Me - AURORA"),
                     evidence: [post-click screenshot, ax-tree-after],
                     failure_layer: None
                   }
                 )}
```

Mapping to existing primitives (so the implementation cost is real):

- `music.search.results` ≈ **`debug.findWindowRows`** (window-scoped
  OCR rows — primary) + Candidate wrapping. `debug.findScreenRows`
  is only a fallback if window resolution itself fails. Window-
  scoped is mandatory here because the QQ音乐 result surface is a
  WebView inside a known window; using screen-scoped OCR invites
  contamination from other windows (the inspect viewer, Chrome,
  another music app) and produces candidates that don't survive
  scrolling or resize.
- `music.result.play` ≈ existing `play_visible_row.v0` recipe steps
  (dismiss search overlay → click row → press play → verify
  now-playing). The new contract layer:
  1. resolves the `candidate_ref` to its source artifact and reads
     out the Candidate
  2. evaluates `liveness.preconditions` against a fresh
     `debug.captureWindow` + re-`findWindowRows` (NOT screen-scoped)
  3. evaluates `control.requires_*` and re-foregrounds /
     re-focuses if needed
  4. delegates the actual press to the existing `play_visible_row`
     step chain

So the first contract-consuming skill is **not** a new recipe
type; it's a typed facade over `play_visible_row.v0` that exposes
candidates from `findWindowRows` and re-verifies preconditions
before delegating to the existing row-click flow.

This is deliberately conservative. The contract's first job is to
prove the **shape** works against a known-validated primitive, not
to be the first place a new behaviour is introduced.

## Implementation plan that follows from this doc

Single commit per step. Total expected: 4 commits.

1. **`feat(contract): add OperationResult / Candidate / VerificationResult Rust types`**
   - New file `src/contract.rs`
   - Types only, no driver wiring
   - Serde derive + minimal tests for round-trip
   - No `confidence`. No hard `valid_until`.
   - Includes `liveness.preconditions` (window + anchor recheck)
     and `control.requires_*` as separate axes.
   - Includes `candidate_ref` as the cross-operation handle
     (NOT bare `candidate_id`).
   - `ArtifactRef` carries `{run_id, artifact_id, span_id,
     captured_event_id}` so callers can resolve a timestamp via
     the event-record path (since `ArtifactRecordV1Alpha1` has no
     timestamp field of its own).
   - `FailureLayer` is the 6-variant enum: `grounding_failed`,
     `candidate_expired`, `control_failed`,
     `verification_unreliable`, `state_changed_no_match`,
     `semantic_mismatch`.

2. **`feat(macos/music): add music.search.results getter`**
   - New driver operation under `macos.desktop` namespace OR a new
     `macos.music` namespace (decide in commit message — leaning
     toward `macos.music` to keep the contract clearly above the
     primitive layer)
   - Wraps **`debug.findWindowRows`** (window-scoped, primary).
     `find_screen_rows` only as a fallback if window resolution
     itself fails, and that fallback path is marked clearly in the
     produced Candidate's `evidence.observation`.
   - Captures `liveness.preconditions` (window_ref + anchor_recheck
     parameters) and `control.requires_*` at getter time so the
     action doesn't have to guess what the getter saw.
   - Emits `OperationResult { output: Candidates(...) }` as a typed
     artifact whose `artifact_id` becomes the `source_artifact_id`
     in any future `candidate_ref`.
   - Recipe `recipes/macos/qqmusic/contract_search.v0.json` + a
     candidate-only case matrix

3. **`feat(macos/music): add music.result.play action consuming candidate_ref`**
   - Action input is a full `candidate_ref` (`source_run_id` +
     `source_span_id` + `source_operation_id` +
     `source_artifact_id` + `candidate_local_id`), not a bare
     local ID.
   - Resolves the candidate by reading the source artifact from
     the store, fails fast if the artifact is missing.
   - Re-verifies `liveness.preconditions` via a fresh
     `debug.captureWindow` + window-scoped `findWindowRows`. On
     failure → `OperationResult { output: Verification(...
     failure_layer: candidate_expired ...) }`.
   - Then evaluates `control.requires_*` (re-foreground / re-focus
     if needed). On failure → `failure_layer: control_failed`.
   - On both passing → delegates to the existing
     `play_visible_row.v0` step chain (don't reinvent activation).
   - Verifier failure modes map onto the FailureLayer enum so
     downstream agents can branch retries correctly.

4. **`docs(contract): freeze v0 contract surface after the music
   loop validates`**
   - Locks the field names so downstream agents can rely on them
   - Lists what the v0 deliberately doesn't include (confidence,
     hard ttl, multi-candidate batch select, cross-app aggregation)

If step 2 or 3 surfaces a contract gap that requires field
additions, those land as **deltas to this doc** before code, and a
fresh hands-off probe is taken if the gap is about actual app
behaviour.

## What this doc does not commit to

- It does not commit to a `macos.music` namespace. The next commit
  decides between `macos.music.*` and `macos.desktop.music.*` based
  on whether a non-music domain (notes, files, system settings)
  would benefit from sharing the same contract surface.
- It does not commit to specific Rust trait shapes. The serde
  structs are the load-bearing surface; trait abstractions can
  come later if multiple drivers need them.
- It does not extend to NetEase Cloud Music. NetEase is the
  obvious second app; we'll learn from QQ音乐 first.

## Sign-off

This doc is the evidence root for the upcoming `src/contract.rs`.
If the Rust types in the next commit deviate from what's described
here without an updated entry above, the deviation is a contract
drift and should be called out.
