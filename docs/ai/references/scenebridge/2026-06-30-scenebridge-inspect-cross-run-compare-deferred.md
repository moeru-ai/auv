# SceneBridge B2c: Inspect viewer cross-run compare — deferred

Date: 2026-06-30
Status: **deferred** (evidence gate — no implementation)
Server API needed: **No**

Defer 期间不需要新 inspect server API、不需要 compare 专用数据模型、不需要
producer / A 线变更。本方向仅记录决策与 reopen 门槛；零 viewer / schema 实现。

## Summary

B2c would let users compare evidence **across two or more runs** in the inspect
viewer (side-by-side or diff-style). That slice is **deferred** until real
workflows prove B2a in-run links and B2b list filters are insufficient.

## Why defer (owner rationale)

- Compare **dimensions** are unvalidated — hard-building compare now is likely
  pseudo-demand.
- [B2a](2026-06-30-scenebridge-inspect-diagnostic-links.md) and
  [B2b](2026-06-30-scenebridge-inspect-list-filter.md) just shipped
  **in-run navigation** and **list filtering**; those must be exercised first.
- Cross-run compare implies field alignment across runs, latest-only aggregation
  ambiguity, and lineage boundaries — higher cost than filter + links alone.

## Prerequisites (shipped)

| Slice | What users have today |
|-------|----------------------|
| **B1** | Proof panel + list badges via `view_parser_summary` |
| **B2a** | In-run diagnostic links (unique-resolve or hide) |
| **B2b** | Client-side list chips: failed / stale / limits (multi-select AND) |

Available data without new producers: list `view_parser_summary` (B1c), detail
`view_parser` / `select_results` / artifacts / spans (B1/B2a).

## Evidence gate (no time threshold)

Reopen is **not** triggered by calendar observation (e.g. “wait 2–4 weeks”).
Time is not auditable. Only **Evidence records** below count.

Guiding questions when recording pain:

| Signal | Question |
|--------|----------|
| **B2b filter** | After filtering failed/stale/limits, do you still manually compare the **same evidence** across 2+ runs? |
| **B2a links** | Does in-run navigation already cover most comparison cost? Do you still open a second run tab to read side-by-side? |
| **List badges** | Are `×N`, outcome, verification, limits badges insufficient — must you see two runs at once? |
| **Lineage** | Does `source_run_id` / `memory_id` trigger parent-vs-child comparison with no entry? (B2a defers cross-run `source_run_id` navigation.) |
| **Workflow** | Is the pain **finding** runs or **comparing** runs? If finding only, prefer B2b extensions (sort, more chips) over compare. |

## Observation template (required 5 fields)

Each Evidence record **must** include all five fields. Incomplete records do
**not** count toward reopen.

| Field | Content |
|-------|---------|
| **run_ids** | Concrete run ids involved (reopen needs ≥2) |
| **b2a_b2b_used** | Which B2a/B2b capabilities were tried (e.g. diagnostic links, failed+stale filter, badge scan) |
| **evidence_to_compare** | Specific evidence to compare side-by-side (field, artifact role, span, limits text, etc.) |
| **why_insufficient** | Why B2a in-run navigation, B2b filter, and/or list sort — alone or combined — still fail |
| **frequency_blocker** | How often (one-off / weekly / every regression) + whether it blocks the workflow |

### Blank template (copy per incident)

```markdown
### Evidence record

- **run_ids**:
- **b2a_b2b_used**:
- **evidence_to_compare**:
- **why_insufficient**:
- **frequency_blocker**:
```

## Reopen criteria (all required for B2c **design** slice only)

Entering a B2c design slice does **not** auto-approve implementation.

1. **≥2** independent Evidence records, each with all 5 fields, and each requiring
   **the same evidence dimension** across 2+ runs at once.
2. Owner confirms `why_insufficient`: pain cannot be solved by stronger B2a
   in-run links, B2b filters, or **list sort** alone.
3. Owner names **one** primary compare dimension (see candidates — others stay
   deferred).
4. `run_ids` + `evidence_to_compare` form a reproducible corpus example.

If not met: stay deferred; prefer B2b enhancements (sort, `not_found` chip, etc.)
over compare.

## Candidate dimensions (hypotheses — not roadmap)

| Dimension | Might compare | Risks / open questions |
|-----------|---------------|------------------------|
| **outcome** | `latest_outcome` across runs | Latest-only; earlier resolution history lost |
| **verification** | `latest_verification_status` | Diverges from run `status_code` semantics |
| **known_limits** | `has_known_limits` + limits text | List is bool-only; detail needs `select_results` |
| **lineage** | `memory_id` / `source_run_id` chain | Cross-run navigation + schema; B2a already defers |
| **full proof** | `resolution_summaries` side-by-side | Heavy UI; two tabs may suffice |

Before any implementation: owner locks **one** primary dimension. No “full
dimension compare v0.”

## Explicit non-goals (while deferred)

- No compare UI, multi-select run bar, or diff engine
- **No server API** — no new `/runs` compare endpoint
- **No compare data model** — no new wire types for cross-run alignment
- **No producer / A-line changes**
- No default clickable `source_run_id` to another run (unless a separate slice
  after reopen)
- No inspect-viewer-v0 full tab reopen

## Related handoffs

- [B1 inspect viewer consumption](2026-06-30-scenebridge-inspect-viewer-consumption.md)
- [B2a diagnostic links](2026-06-30-scenebridge-inspect-diagnostic-links.md) — landed
- [B2b list filter](2026-06-30-scenebridge-inspect-list-filter.md) — landed

## Validation (this deferral doc only)

```sh
git diff --check
```
