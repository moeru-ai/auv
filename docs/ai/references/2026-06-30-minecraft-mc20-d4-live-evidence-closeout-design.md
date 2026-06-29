# Minecraft MC-20 D4: Live evidence closeout design

Date: 2026-06-30

Status: **D4 closed** — graduation scope for canonical CLI Layer-3 operator
evidence matrix G0–G8. Live closeout recorded 2026-06-30.

## One-line summary

MC-20 D4 **graduates** the canonical CLI `auv minecraft query-wired-live-click`
operator evidence chain by unifying verdict documentation across G0–G8, adding the
sole remaining live gate **G8 `absent`** (`attempted=true` + dispatch failed), and
recording honest limits. D4 does **not** treat post-dispatch-success `absent` as
closeout success.

## Graduation scope

| In scope | Out of scope |
| --- | --- |
| D4 design + live closeout graduation doc | `run_read.rs` / Core-C3 mapper edits |
| G8 live: `attempted=true`, `click_summary` absent → `verifications` empty → `absent` | Post-dispatch-success still `absent` (bug / FAIL, not evidence) |
| Cross-reference D2.1 / D2.2 / D3 / D3.1 evidence | MC-20 controller / planner |
| Review hardening: G5+`expected_item_id` discipline test, D2.2 doc | Real Minecraft break/harvest gameplay proof |
| macOS canonical CLI + `inspect --store-root` excerpts | osu CLI symmetry |

## G8 absent discipline (owner boundary)

Producer gate: `query_wired_dispatch_succeeded` = `click_summary.is_some()`
(`src/verticals/minecraft/verification.rs`). Layer-3 post-action verification runs
only when `attempted=true` **and** dispatch succeeded.

| Condition | Legitimate `verification_outcome` | D4 evidence gate |
| --- | --- | --- |
| `attempted=false` | `not_attempted` | G2 / G3 (D2.1) |
| `attempted=true` && `click_summary` absent | Layer-3 skipped; `verifications` empty | **`absent` — G8 (D4 live)** |
| `attempted=true` && `click_summary` present | `unreliable` / `inconclusive` / `passed` / `failed` | G4–G7 (D2.1 / D3) |
| `attempted=true` && dispatch succeeded && `verifications` still empty | **Non-target** | Closeout **FAIL** / anomaly |

**G8 is the only live `absent` success state.** Dispatch failure does not map to
verification `failed` (Core-C1 unchanged). MC-19 D4 known limit may remain on
`operation-result` when dispatch fails.

**Forbidden closeout pattern:** `click_summary` present **and**
`verification_outcome=absent` — indicates projection or producer bug, not graduated
evidence.

## Full evidence matrix G0–G8

| Gate | ID | Outcome / type | Source doc | Layer-3 `verifications` |
| --- | --- | --- | --- | --- |
| Parse auto | G0 | — (`cargo test parse_minecraft_query_wired_live_click`, 11) | D2.1 | — |
| Parse negative | G1 | CLI exit 1 | D2.1 | — |
| Refusal | G2 | `not_attempted` | [D2.1](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md) | empty |
| Not consumable | G3 | `not_attempted` | D2.1 | empty |
| Click, no witness | G4 | `unreliable` | D2.1 | 1× `VerificationUnreliable` |
| Click + tick witness | G5 | `inconclusive` | D2.1 | 1× `state_changed`, `semantic_matched: None` |
| Semantic pass | G6 | `passed` | [D3 live](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md) | 1× `semantic_matched: true` |
| Semantic fail | G7 | `failed` | D3 live | 1× `semantic_matched: false` |
| Dispatch failed | G8 | `absent` | [D4 live closeout](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md) | **empty** |

Design references:

- D1 producer table:
  [`2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`](2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md)
- D2 CLI entry:
  [`2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`](2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md)
- D3 semantic pass/fail:
  [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md)

### G5 discipline (D3 regression)

When `--verification-expected-item-id` is set, tick-advance-only witnesses (G5
shape) must resolve to **`failed`**, not G5 `inconclusive`. Covered by integration
test `query_wired_live_action_with_expected_item_id_tick_advance_projects_failed`
in `src/verticals/minecraft/mod.rs`.

## D3.1 freshness (implemented, referenced in closeout)

Post-action verification reads a **newer** post frame via
`read_latest_spatial_frame_newer_than` with bounded wait
`MC20_POST_FRAME_WAIT` (`TailFrameWaitConfig::new(750, 25)` in
`src/verticals/minecraft/verification.rs`).

- Synthetic D3 G6/G7 semantics unchanged.
- Live telemetry seam hardened; integration test
  `query_wired_live_action_waits_for_fresher_post_frame` covers bounded wait.
- D3.1 does **not** by itself prove full gameplay harvest success.

## D2.2 inspect hint gate

`inspectHint` prints only when **both**:

1. `query_wired_verification_readable(wiring)` — dispatch succeeded (`click_summary`
   present) and `attempted=true`.
2. `should_write_local(inspect)` — `--inspect-local-write` is not `false`.

G8 dispatch-failed runs therefore **omit** `inspectHint` (no readable verification
in store from Layer-3). Operators use explicit `auv inspect <runId> --store-root`.

## G8 live strategy

Reproducible dispatch failure without stubbing the executor:

- `click_ready` path + deliberate wrong `--target-title` → window resolve / invoke
  `Err` → `attempted=true`, `click_summary=None`.

Canonical command template documented in
[`2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md).

## Blast radius (`invoke_click_at_window_point` Failed → `Err`)

`click_summary_from_invoke_result` (via `invoke_click_at_window_point`) is shared
beyond MC-20 `query-wired-live-click`:

- `src/main.rs` `run_minecraft_live_click` — failed invoke now surfaces as
  command `Err` instead of a misleading success summary string.
- `src/verticals/osu/query_live_action.rs` — same helper path; osu dispatch-failed
  live evidence is **not** a D4 closeout gate but behavior changed consistently.

MC-20 G8 closure depends on this mapping so dispatch failure leaves
`click_summary` absent and Layer-3 verification `absent`.

## Paused after D4 (observation only)

- MC-20 controller / planner / action lease
- Real Minecraft break/harvest gameplay witnesses beyond synthetic shaping
- osu `query-wired-live-click` CLI symmetry
- Post-dispatch-success `absent` investigation (only if observed — not a closeout gate)

## Related

- D4 live graduation:
  [`2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`](2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md)
- MC-20 final pause decision:
  [`2026-06-30-minecraft-mc20-final-closeout-pause-decision.md`](2026-06-30-minecraft-mc20-final-closeout-pause-decision.md)
- D2.1 G0–G5:
  [`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md)
- D3 G6/G7:
  [`2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`](2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md)
