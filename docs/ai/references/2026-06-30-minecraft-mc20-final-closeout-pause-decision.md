# Minecraft MC-20 final closeout / pause decision

Date: 2026-06-30

Status: **final closeout + pause decision** — MC-20 is closed for its approved
canonical CLI Layer-3 verification scope. This note records what landed from D1
through D4, which evidence counts as closed, which interpretations are
forbidden, and which owner-named triggers would be required to reopen the lane.
**No new implementation is approved by this note.**

## One-line summary

MC-20 already closes the Minecraft **canonical CLI** query-wired Layer-3 chain
for `not_attempted` / `absent` / `unreliable` / `inconclusive` / `passed` /
`failed`, with D3.1 hardening the post-frame freshness seam and D4 freezing the
G0–G8 evidence matrix. What remains is **not** “the obvious next controller
step”; it is a deliberate pause boundary.

## Scope boundary

**In scope:**

- Final inventory of what D1 → D4 actually landed
- Unified interpretation of the canonical CLI evidence matrix
- Explicit pause boundary for controller / planner / lease / Core-C3 reopen
- Reopen triggers for future owner-named slices
- Anti-misread rules for `absent`, synthetic witnesses, and “almost controller”

**Out of scope:**

- New code changes
- `run_read.rs` / inspect mapper expansion
- MC-20 controller / planner / action lease
- osu CLI symmetry or multi-vertical admission runtime
- Real gameplay harvest-success proof
- Core-B / Core-C3 / Core-D reopening by implication

This is a **boundary record**, not a proposal to continue implementation.

## Closed phases

These phases are complete for their named scope. “Closed” here means the slice
reached its intended endpoint and should not be reopened unless a new owner
slice names exactly what changed.

| Phase | Closure type | Pointer | What “closed” means |
| --- | --- | --- | --- |
| **D1** | **Design + producer seam landed** | [`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md) | Post-action verification seam exists on the MC-19 query-wired path; Layer-3 runs only after dispatch success |
| **D2** | **Canonical CLI landed** | [`2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`](2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md) | `auv minecraft query-wired-live-click` is the canonical operator entry; `main.rs` stays thin |
| **D2.1** | **Live closure** | [`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md) | G0–G5 closed on the canonical CLI (`not_attempted`, `unreliable`, `inconclusive`) |
| **D2.2** | **Inspect/store-root closure** | [`2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`](2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md) | `inspect --store-root` and `inspectHint` gate are aligned with producer truth |
| **D3** | **Semantic pass/fail landed** | [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md), [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md) | G6/G7 `passed` / `failed` producer evidence closed via `expected_item_id` |
| **D3.1** | **Freshness hardening landed** | [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md) | Post-action verification waits for a **newer** post frame before evaluating world diff |
| **D4** | **Graduation closeout** | [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md), [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md) | G0–G8 matrix frozen; G8 `absent` is closed only for dispatch-failed Layer-3 skip |

## What MC-20 now proves

### Canonical CLI surface

The approved MC-20 operator surface is:

```text
auv minecraft query-wired-live-click
  → query
  → readiness / admission
  → dispatch
  → post-action verification
  → operation-result
  → inspect / read-side projection
```

The chain is now closed on the **canonical CLI**, not only on examples or test
helpers.

### Verified outcome coverage

| Outcome | Status | Evidence source |
| --- | --- | --- |
| `not_attempted` | **closed** | D2.1 G2 / G3 |
| `absent` | **closed with narrow boundary** | D4 G8 only (`attempted=true` + dispatch failed + Layer-3 skipped) |
| `unreliable` | **closed** | D2.1 G4 |
| `inconclusive` | **closed** | D2.1 G5 |
| `passed` | **closed** | D3 G6 |
| `failed` | **closed** | D3 G7 |

### What the evidence is actually about

MC-20 evidence is about **Layer-3 post-action witness honesty** on the canonical
CLI path. It is **not** evidence that Minecraft gameplay tasks are solved in the
large, and it is **not** a controller/runtime graduation note.

## Frozen interpretation rules

These rules are now part of the pause boundary.

### 1. `absent` is narrow, not generic

`verification_outcome=absent` is a valid closeout success state **only** when:

- `attempted=true`, and
- dispatch failed (`click_summary` absent), and
- Layer-3 verification was therefore skipped by design.

The closed evidence for that state is D4 G8.

**Forbidden reading:**

> “dispatch succeeded but `verification_outcome=absent`, so that still counts as
> MC-20 success.”

No. Post-dispatch-success `absent` is an **anomaly / fail**, not graduated evidence.

### 2. Synthetic witness closure stays synthetic

G5 / G6 / G7 prove semantic plumbing and projection honesty using synthetic
pre/post witnesses. They do **not** prove full real gameplay break/harvest
success.

**Forbidden reading:**

> “D3 + D3.1 means MC-20 already proved real Minecraft harvest success.”

No. D3 closes `passed` / `failed` production semantics; D3.1 hardens post-frame
freshness. Neither one is a gameplay proof.

### 3. D3.1 is seam hardening, not a new runtime lane

D3.1 only makes the live telemetry seam less brittle by waiting for a newer post
frame. It does not introduce controller logic, planner logic, lease logic, or a
new read-side vocabulary.

### 4. Canonical CLI closure does not imply controller readiness

The fact that the canonical CLI path is closed does **not** mean MC-20 is “just
one step away” from controller / planner / lease work.

The current closed lane is:

```text
single vertical
+ single operator entry
+ single dispatch attempt
+ single post-action verification seam
```

That is materially narrower than controller/orchestration work.

## Continue defer

These lanes stay frozen by default.

| Deferred slice | Why it remains deferred |
| --- | --- |
| **MC-20 controller / planner** | Outside the approved MC-20 scope; no owner approval in any D1–D4 note |
| **Action lease / ownership runtime** | Separate lane from operator closeout; not unlocked by canonical CLI evidence |
| **Core-C3 reopen** | Read-side projection is already sufficient for MC-20’s approved scope |
| **osu CLI symmetry** | Separate vertical slice; D4 only records blast radius honesty |
| **Real gameplay harvest-success proof** | Distinct evidence problem; not implied by synthetic Layer-3 closure |
| **Post-dispatch-success `absent` investigation** | Only reopen if it is actually observed; not part of graduated success |

## Reopen triggers

A paused lane does not reopen because it “feels next.” It reopens only when the
owner names the trigger **and** the exact slice.

| Trigger | Unlocks (candidate only) | Does **not** auto-unlock |
| --- | --- | --- |
| **Observed real gameplay requirement** | A named live-gameplay evidence slice | Controller / planner / lease |
| **Observed post-dispatch-success `absent` anomaly** | A bug or evidence-repair slice | Reclassifying `absent` as acceptable |
| **Owner explicit orchestration approval** | A named MC-20 controller/planner slice | Core-C3, Core-B, osu symmetry |
| **Need for cross-vertical CLI symmetry** | A named osu or generic admission slice | Retroactive widening of MC-20 |

**Trigger met ≠ implement.** A reopened lane still needs a named slice and fresh
scope review.

## Anti-misread rule

This is the main point of the note.

> **MC-20 final closeout means “the approved operator-evidence lane is done.”**
> It does **not** mean “controller is the obvious next implementation.”

### Forbidden misreads

The following readings are explicitly rejected:

- “D4 is done, so MC-20 should now grow a controller.”
- “Canonical CLI closure means we can widen into planner/lease without another owner decision.”
- “D3.1 freshness hardening is basically the start of a generic runtime.”
- “`absent` is now always acceptable whenever the run reaches dispatch.”
- “Since osu shares some glue, MC-20 final closeout should automatically pull osu in.”

### Allowed readings

The following are allowed:

- MC-20 is closed for its approved canonical CLI Layer-3 evidence scope.
- The six read-side outcomes are covered on the Minecraft query-wired path.
- Dispatch-failed `absent` is closed; post-dispatch-success `absent` is not.
- Real gameplay proof, controller work, and cross-vertical symmetry remain separate owner slices.

## Recommended status after this note

MC-20 should now be treated as:

- **closed** for the approved D1 → D4 scope,
- **paused** for further implementation,
- and **reopenable only by owner-named slice**.

That is the intended endpoint.

## Related

- D1 seam:
  [`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md)
- D2 canonical CLI:
  [`2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`](2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md)
- D2.1 live closure:
  [`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md)
- D3 semantic pass/fail:
  [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md)
- D3 live closure:
  [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md)
- D4 design:
  [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md)
- D4 graduation:
  [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md)
- Core-C3 read-side projection boundary:
  [`2026-06-30-auv-core-c3-d2-verification-outcome-read-side-projection.md`](2026-06-30-auv-core-c3-d2-verification-outcome-read-side-projection.md)
- Core-D1 pause boundary:
  [`2026-06-30-auv-core-d1-action-lease-ownership-boundary-review.md`](2026-06-30-auv-core-d1-action-lease-ownership-boundary-review.md)

## One-sentence summary

MC-20 is finished for the approved canonical CLI Layer-3 evidence lane; what
remains is **not** “the next obvious controller step,” but a deliberately paused
set of separate owner-gated slices.
