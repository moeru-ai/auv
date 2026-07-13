# 2026 06 30 Minecraft Probe 20 Reference

Durable reference folded from intermediate handoffs along the same responsibility line. Historical slice codes are omitted from navigation; see absorbed sources below.

## Status

Merged closeout / landed reference. Prefer this document over the absorbed intermediate notes.

## Absorbed sources

- **Minecraft MC-20 D1: Query-wired post-action semantic verification** — formerly `2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`
- **MC-20 D2.1: Canonical CLI live closure** — formerly `2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`
- **Minecraft MC-20 D2: Query-wired live click CLI entry** — formerly `2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`
- **Minecraft MC-20 D3: Query-wired Layer-3 passed/failed semantic closure** — formerly `2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`
- **MC-20 D3: Layer-3 passed/failed live closure** — formerly `2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`
- **Minecraft MC-20 D4: Live evidence closeout design** — formerly `2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md`
- **MC-20 D4: Live evidence closeout (graduation)** — formerly `2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`
- **Minecraft MC-20 final closeout / pause decision** — formerly `2026-06-30-minecraft-mc20-final-closeout-pause-decision.md`

## Folded notes

### Minecraft MC-20 D1: Query-wired post-action semantic verification

_Source: `2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`_

Date: 2026-06-30 Status: **D1 implemented; D1.1 hardening landed** — closes minimal Layer 3 post-action semantic verification on the MC-19 `query-wired live click` chain. MC-20 orchestration/controller lane remains **paused** after this slice.

### MC-20 D2.1: Canonical CLI live closure

_Source: `2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`_

Date: 2026-06-30

### Minecraft MC-20 D2: Query-wired live click CLI entry

_Source: `2026-06-30-minecraft-mc20-d2-query-wired-live-click-cli-design.md`_

Date: 2026-06-30 Status: **D2 implemented** — stable vertical CLI for the MC-19+MC-20 D1 library chain. **D2.1 live closure recorded**; **D2.2 inspect/store-root closure**; **D3 semantic pass/fail closure**; **D4 live evidence closeout (G0–G8) closed** (2026-06-30) — see [`2026-06-30-minecraft-mc20-d2-1-canonical-cli-live-closure.md`](2026-06-30-minecraft-probe-20-reference.md), …

### Minecraft MC-20 D3: Query-wired Layer-3 passed/failed semantic closure

_Source: `2026-06-30-minecraft-mc20-d3-semantic-pass-fail-closure-design.md`_

Date: 2026-06-30 Status: **D3 implemented** — producer chain wires `expected_item_id` through `QueryWiredPostActionWitness` so live `query-wired-live-click` can emit honest Layer-3 `VerificationResult` claims with `semantic_matched: Some(true/false)` and read-side `verification_outcome` `passed` / `failed`. Synthetic witness fixtures close G6/G7; no `run_read` mapper changes. **D4 graduated** full…

### MC-20 D3: Layer-3 passed/failed live closure

_Source: `2026-06-30-minecraft-mc20-d3-semantic-pass-fail-live-closure.md`_

Date: 2026-06-30

### Minecraft MC-20 D4: Live evidence closeout design

_Source: `2026-06-30-minecraft-mc20-d4-live-evidence-closeout-design.md`_

Date: 2026-06-30 Status: **D4 closed** — graduation scope for canonical CLI Layer-3 operator evidence matrix G0–G8. Live closeout recorded 2026-06-30.

### MC-20 D4: Live evidence closeout (graduation)

_Source: `2026-06-30-minecraft-mc20-d4-live-evidence-closeout.md`_

Date: 2026-06-30

### Minecraft MC-20 final closeout / pause decision

_Source: `2026-06-30-minecraft-mc20-final-closeout-pause-decision.md`_

Date: 2026-06-30 Status: **final closeout + pause decision** — MC-20 is closed for its approved canonical CLI Layer-3 verification scope. This note records what landed from D1 through D4, which evidence counts as closed, which interpretations are forbidden, and which owner-named triggers would be required to reopen the lane. **No new implementation is approved by this note.**

## Evidence / follow-ups

Open the absorbed source tombstones only if you need the pre-merge wording. Update new work against this merged note and the owning folder `INDEX.md`.
