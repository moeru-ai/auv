# Phase 3 — First Contract-Consuming Skill (Design)

Date: 2026-05-21

Status: design / pre-implementation

## Why

Phase 2 froze the press/presentation semantics on the driver side
(`cursorDisturbance`, `pressMechanism`, `smartPress.strategy`, the overlay
lifecycle fields, the OCR→AX projection points). The contract is real on
the driver side but **no skill manifest currently consumes any of those
fields**. Until one does, the Phase 2 freeze is doc-only.

Phase 3 priority #1 from the freeze doc was:

>把 `cursorDisturbance` 提到 skill contract 一级字段。下游 skill 应该能声
> 明 "I require `cursorDisturbance=none`"，case matrix 应该能在不满足时直
> 接 fail，而不是依赖人工读 artifact。

This design note picks the smallest verifiable target and writes the path.

## Survey of the schema

Reading `src/skill.rs:1927-1957` (`enforce_step_expectations_*`):

- `expect.signal_equals` and `expect.signal_contains` already accept
  arbitrary signal keys / values.
- So a recipe step CAN already write:

  ```json
  "expect": {
    "signal_equals": {
      "cursorDisturbance": "none",
      "pressMechanism": "ax-action",
      "performedAction": "AXPress"
    }
  }
  ```

  and the case will fail if the driver returns anything else.

- This means **no schema change is required to consume the Phase 2
  contract**. The first contract-consuming skill is purely a recipe-level
  change.

## Target

`macos.notes.create_and_verify_note.v0` (`recipes/macos/notes/create-and-verify-note.v0.json`)
is the cleanest fork target:

- Already validated end-to-end (case `notes-marker-baseline`).
- Already uses `debug.pressButton` against an AX-discoverable Notes
  toolbar button ("新建备忘录"). Pressing this button is the highest-
  pointer-disturbance step in the recipe and is the easiest to swap to
  `debug.axPressButton`.
- Verification is already AX-based (`debug.verifyAxText`), not screenshot
  OCR — so the contract win is concentrated on the activation step, not
  the verification step.

## The smallest verifiable win

Create `macos.notes.create_and_verify_note.v1` with two changes against
v0:

1. `create-note` step:
   - `command_id`: `debug.pressButton` → `debug.axPressButton`
   - `disturbance.classes`: `[foreground_app, keyboard, pointer]` →
     `[foreground_app, keyboard]`
   - `disturbance.max`: `pointer` → `keyboard`
   - Add `expect.signal_equals`:
     - `cursorDisturbance: "none"`
     - `pressMechanism: "ax-action"`
     - `performedAction: "AXPress"`

2. Recipe-level `disturbance_policy`:
   - Leave `max_disturbance: "pointer"` for now (focus-body still uses
     pointer; see "What this win does not cover" below).
   - Add a note explaining that `create-note` is now contract-asserted at
     keyboard disturbance, while the recipe-level budget is still pointer
     because `focus-body` has not been migrated.

The case matrix gets a sibling case (`notes-marker-ax-press`) referencing
v1 so the existing v0 baseline stays as a regression anchor.

## What this win actually proves

- The recipe runner reads `expect.signal_equals.cursorDisturbance` and
  fails when the driver does not emit it.
- A real narrow skill can be re-grounded on a Phase 2 primitive without
  schema changes.
- The "press a Notes toolbar button" sub-task can be made
  pointer-free **today**, with audit evidence in the trace.

## What this win does NOT cover

Explicitly accepted boundaries (these become the Phase 3 #2 / #3 work
items):

- `focus-body` still goes through `debug.focusTextInput`, which internally
  calls `click_point`. So the recipe top-level disturbance budget cannot
  drop below `pointer` until a `debug.axFocusTextInput` primitive exists
  (using `AXUIElementSetAttribute(focused, true)`).
- `paste-note-body` still requires the clipboard.
- `activate-target-app` still requires foregrounding Notes.

So this win narrows the disturbance on ONE step only. The product-level
"this whole recipe is keyboard-only" claim is **not** earned by this
change and must not be advertised as such.

## Phase 3 follow-on backlog (after this win lands)

In rough order:

1. ~~**`debug.axFocusTextInput` driver primitive**~~ — DONE
   (`feat(macos): add debug.axFocusTextInput command`). Wraps
   `AXUIElementSetAttributeValue(kAXFocusedAttribute, true)` and ships
   as `macos.notes.create_and_verify_note.v2`, the first narrow skill
   with a fully cursor-warp-free activation chain (recipe-level
   disturbance = clipboard).
2. **Add `ax-perform-action-clipboard-paste` to `SkillActivation`** —
   the v2 recipe still labels its activation taxonomy
   `pointer-focus-clipboard-paste` because the enum is closed. Once the
   schema migration lands, v2's taxonomy label can become honest at
   the recipe header level too.
3. ~~**Recipe-level disturbance assertion**~~ — DONE
   (`feat(skill): enforce recipe disturbance budget at manifest
   validation`). `validate_skill_disturbance_budget` is now part of
   `validate_skill_manifest_with_commands`, so `skill list`,
   `skill cases run --dry-run`, and bundle verify all reject recipes
   where any step's `disturbance.max` exceeds the recipe's
   `disturbance_policy.max_disturbance` or where any step's class is
   not in `disturbance_policy.declared_classes`. Test count 200 -> 203.
4. **Cross-app case matrix for smartPress fallback** — drive smartPress
   against TextEdit / Notes (AX wins) and a Chromium web view (AX fails →
   pointer fallback) so the fallback path has real data, not just doc
   acknowledgment.
5. **QQ音乐 row-fallback re-grounding** — once the primitives above
   exist, try to migrate the QQ音乐 row-fallback recipe to smartPress.
   If AX genuinely doesn't reach music rows, that becomes documented
   evidence, not a guess.

## Discovered during dry-run

`SkillStrategy.activation` is a closed enum
(`src/skill.rs:247-260` + the allowed taxonomy table at `:140-180`).
Inventing `ax-perform-action-clipboard-paste` for v1 fails parsing.

This is actually the right outcome for v1: the activation chain still
runs `debug.focusTextInput` (pointer warp) for `focus-body`, so the
honest taxonomy label remains `pointer-focus-clipboard-paste`. The
contract-consumption win lives at the per-step `expect.signal_equals`
level, not the taxonomy level.

Add `ax-perform-action-clipboard-paste` as a new `SkillActivation`
variant **only after** `debug.axFocusTextInput` lands and the whole
chain is genuinely cursor-warp-free.

## Non-goals for this design

- Not changing any field name frozen in the Phase 2 freeze.
- Not adding new schema fields. The win is to prove the schema is already
  enough.
- Not re-grounding QQ音乐 in this pass — Notes is the cheaper first
  consumer because its activation button is genuinely AX-pressable.
- Not introducing a new driver primitive in this pass. That is a separate
  follow-on commit.
