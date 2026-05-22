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

- `docs/ai/references/2026-05-15-qqmusic-macos-capability-probe.md` —
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
| `candidate_id` | stable string within one `OperationResult` | agents need a handle to pass to actions | core contract requirement |
| `kind` | `SearchResultRow` (enum variant) | distinguishes from `SearchInputField`, `PlayerControl`, etc. — same getter could return different kinds | probe shows several AX/OCR layers exist |
| `label` | human-readable string | for logs / viewer rendering / model prompts | `"Cure For Me"` is the OCR row text from the validated case |
| `target_spec.grounding` | `ocr-anchor` (not `ax-node`) | AX rows are not stable on QQ音乐 results | probe section "AX Tree Findings" + "Result Selection Phase" |
| `target_spec.anchor_text` | the OCR string that identifies the row | this is what `click_screen_text` consumes downstream | `recipes/macos/qqmusic/play-visible-anchor.v0.json` step `resolve-ocr-anchor` |
| `target_spec.region_hint` | optional `{left,top,right,bottom}` ratios | OCR needs region constraint to disambiguate row text from chrome | recipe `selection_region_*_ratio` defaults |
| `evidence.artifact_ref` | `{run_id, artifact_id}` pointing at the source screenshot | proves the candidate was real at capture time + lets viewer overlay-render it | every existing recipe captures evidence; this just makes the back-reference explicit |
| `evidence.captured_at_millis` | timestamp | half of the liveness story | timestamp already in every artifact |
| `evidence.observation` | the OCR match shape (text, bounds, confidence, match_index) | what the candidate physically was | already produced by `debug.findScreenText` |
| `liveness.invalidation_preconditions` | structured (see below) | candidates die; we must declare how | NEW contract field — no recipe captures this today |
| `liveness.ttl_hint_ms` | optional u64 | advisory upper bound, NOT authoritative | NEW |

### `liveness.invalidation_preconditions` — the field that's new

This is the field the user called out as missing from the original
proposal. Without it, candidate IDs become "try and pray" handles.
For QQ音乐 search rows, the preconditions that need to hold for a
candidate to still be valid:

```
{
  window_ref: { app_bundle_id, window_title_substring | window_number },
  app_frontmost: bool,         // QQ音乐 must still be the frontmost app
  anchor_recheck: {            // re-run findScreenText with the same anchor
    text: <candidate.target_spec.anchor_text>,
    region_hint: <candidate.target_spec.region_hint>,
    expected_min_confidence: <number>,
    max_pixel_distance: <number>  // how far the anchor is allowed to have moved
  }
}
```

Action handlers ingesting a `candidate_id` MUST re-evaluate these
before acting. If any fails:

- the action returns an `OperationResult` with `failure_layer:
  candidate_expired` and an `invalidation_evidence` artifact
  (re-capture + diff) — it does NOT try to "guess what the user
  meant" and click anyway

This is the load-bearing piece that turns candidates from a
hopeful indirection into a hard precondition.

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
| `failure_layer` | `Option<VerificationFailureLayer>` | enum: `not_executed`, `executed_no_state_change`, `state_changed_no_match`, `candidate_expired`, `verification_unreliable` |
| `evidence` | `Vec<ArtifactRef>` | verifier's own evidence (e.g. post-action screenshot + AX now-playing capture) |
| `observed_label` | `Option<String>` | what we actually observed (e.g. `"天空仍灿烂"` when expecting `"晴天"` — directly maps to the Phase 1 unresolved Chinese boundary) |

`failure_layer` is what makes the verifier non-binary. It's the
field that lets the next agent decide whether to retry with a new
candidate (`candidate_expired`), back off the recipe entirely
(`verification_unreliable`), or accept a documented
state-change-without-semantic-match (`state_changed_no_match` →
the Phase 1 Chinese boundary case).

## The narrow end-to-end loop this contract has to serve

The contract's first test is a single end-to-end music loop:

```
1. getter:  music.search.results(query="aa")
              -> OperationResult { output: Candidates([
                   Candidate { id: "row#1", label: "Cure For Me", ... },
                   Candidate { id: "row#2", label: "Soft Universe", ... },
                   ...
                 ])}

2. action:  music.result.play(candidate_id="row#1")
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

- `music.search.results` ≈ `debug.findScreenRows` (already exists,
  returns OCR rows in a region) + Candidate wrapping. Need to add
  the `liveness.invalidation_preconditions` capture.
- `music.result.play` ≈ existing `play_visible_row.v0` recipe steps
  (dismiss search overlay → click row → press play → verify
  now-playing). The new contract layer just typed-wraps the
  invocation chain and adds the candidate liveness recheck.

So the first contract-consuming skill is **not** a new recipe
type; it's a typed facade over `play_visible_row.v0` that exposes
candidates from `findScreenRows` and re-verifies preconditions
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
   - No `confidence`. No hard `valid_until`. Includes
     `liveness.invalidation_preconditions`.

2. **`feat(macos/music): add music.search.results getter`**
   - New driver operation under `macos.desktop` namespace OR a new
     `macos.music` namespace (decide in commit message — leaning
     toward `macos.music` to keep the contract clearly above the
     primitive layer)
   - Wraps `find_screen_rows` + builds Candidates with
     invalidation preconditions
   - Emits `OperationResult { output: Candidates(...) }` as a typed
     artifact
   - Recipe `recipes/macos/qqmusic/contract_search.v0.json` + a
     candidate-only case matrix

3. **`feat(macos/music): add music.result.play action consuming candidate_id`**
   - Re-verifies liveness preconditions before any user-visible
     action
   - On precondition failure → `OperationResult { output:
     Verification(... failure_layer: candidate_expired ...) }`
   - On success → delegates to the existing
     `play_visible_row.v0` step chain (don't reinvent activation)

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
