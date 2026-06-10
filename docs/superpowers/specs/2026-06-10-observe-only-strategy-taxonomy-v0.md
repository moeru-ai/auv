# Observe-Only Strategy Taxonomy v0

Date: 2026-06-10

Status: landed with the first consumer (`sts.readPlayerHp.v0`)

Base: `docs/ai/references/2026-06-10-sts-zero-ax-observe-probe-evidence.md`
creak #1 (no observe-only taxonomy; all allowed combinations were
action-shaped).

## Decision

Add exactly one allowed strategy combination to
`SkillStrategyTaxonomy::allowed()` in `src/skill/mod.rs`:

```text
observe.visual-row.none.capture-evidence
```

backed by two new enum values:

- `SkillStrategyFamily::Observe` (`"observe"`): every step is
  observation/verification; the recipe has no activation path and must never
  grow one.
- `SkillActivation::None` (`"none"`): only meaningful with the observe
  family; the combination gate keeps `none` out of action families and keeps
  pointer/keyboard activations out of the observe family
  (regression-tested both ways).

`verificationContract` reuses the existing `captureEvidence` value: an
observe recipe verifies by recorded capture/recognition evidence plus
step-level `expect` signals. No new contract value, no new schema.

## Why This Shape

- The zero-AX game family's read commands (`sts.read*`) are honest
  observe-only recipes; before this change `validate_skill_manifest` rejected
  any non-action strategy, so they could not exist as recipes at all.
- One combination, one consumer: only `visual-row` grounding is admitted
  because the first consumer grounds through OCR row bands
  (`debug.observeWindowRegion`). `observe.ocr-anchor.*` and detector-backed
  groundings can be added when a real recipe needs them, not before.
- Disturbance stays the recipe's own declaration (`max_disturbance: none` for
  the first consumer); the taxonomy does not encode disturbance.

## Deferred On Purpose

- `observe.ocr-anchor.none.capture-evidence` and detector-grounded observe
  combinations (no consumer yet).
- A recognition-specific verification contract value (would only matter once
  a typed read consumer can assert value formats; see the consumer-seam
  note's deferred `RecognitionItemRef` reasoning).
- Signal export of row text from `debug.observeWindowRegion` (today the value
  lives in the RecognitionResult artifact; expects can only see
  `rows.count`/`rows.visible`).
