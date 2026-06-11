# AUV Phase 1 Freeze

Date: 2026-05-18

Status: accepted historical freeze decision; bundle execution was retired on
2026-06-11

## Decision

Phase 1 is frozen.

That does **not** mean every behavior is solved. It means the first product
slice is now explicit enough that the team should stop relitigating the same
grounding loop and start treating the current boundaries as real inputs to the
next phase.

## What Phase 1 Actually Delivered

- macOS runtime / driver / recipe / case-matrix / bundle / package flow was in
  place during phase 1. The active bundle command surface was retired later.
- QQ音乐 has two narrow playback strategies:
  - OCR-anchor playback
  - row-fallback playback
- Notes and TextEdit exist as native-app AX-text samples.
- bundle export and package verify carried coverage truth, not just recipe
  files.
- member-level coverage summaries now distinguish activation status from
  semantic-selection status.

## Freeze Criteria

Phase 1 is considered complete enough because:

1. There is a stable execution model, not just local shell glue.
2. There are product-facing entrypoints:
   - `skill run`
   - `skill cases run`
   - `skill cases report`
   - historical bundle validation and package commands
3. The validated sample set is durable and inspectable.
4. The remaining failures are explicit boundaries, not hidden contradictions.

## Accepted Unresolved Boundary

The main unresolved boundary accepted into the freeze is:

- QQ音乐 Chinese requested-title semantic selection remains unproven for the
  row-fallback path.

More concretely:

- row fallback on the Chinese result page is validated as an activation path
- but it is **not** validated as “play the exact requested Chinese song”

This is why the bundle now records:

- `activationStatus = validated`
- `semanticSelectionStatus = not-validated`

for the QQ音乐 row-fallback member.

## What This Freeze Does Not Mean

Do not twist this freeze into false product claims.

It does **not** mean:

- QQ音乐 is broadly solved
- Chinese QQ音乐 playback is fully solved
- cross-app distillation is solved
- cross-platform reuse is solved

It means the first narrow product slice is real enough to stop pretending it is
still just a prototype, while still refusing to lie about the parts that are
not done.

## What Phase 2 Should Focus On

Phase 2 should start from productization and contract extraction, not from
reopening the same phase-1 exploration loop.

Priority order:

1. productize the existing QQ音乐 narrow skills further
2. extract a more generic skill contract from real narrow skills
3. only then widen to more apps or platforms

If phase 2 immediately collapses back into “let’s just chase one more OCR edge
case”, that is not progress. That is avoidance.
