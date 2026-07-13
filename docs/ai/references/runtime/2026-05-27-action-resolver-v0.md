# Action Resolver v0

Status: accepted narrow implementation note

## Why this exists

AUV already has multiple ways to act on a grounded UI target:

- AX action: `debug.axPressButton`, `debug.axClickWindowText`
- AX focus: `debug.axFocusTextInput`
- keyboard / clipboard input: `debug.pressKey`, `debug.pasteTextPreserveClipboard`
- pointer fallback: `debug.clickWindowText`, `debug.clickWindowRow`, `debug.clickPoint`

Without a resolver layer, each command records its own partial truth. Agents
then see a mixture of `pressMechanism`, `cursorDisturbance`, `smartPress.*`,
and ad-hoc notes instead of one decision object.

Action Resolver v0 is the first small policy layer that makes the selected
method and fallback reason explicit.

## v0 scope

v0 deliberately does **not** introduce a new production command or recipe
orchestrator. It formalizes the existing `debug.smartPress` path:

```text
query
  -> try ax_click_window_text
  -> if it succeeds: selected_method = ax-action
  -> if it fails and pointer fallback is allowed: click_window_text
  -> if fallback succeeds: selected_method = pointer-click
  -> record policy, selected method, fallback reason, disturbance, evidence
```

This keeps the implementation grounded in an already-tested debug command.

## Contract

Every resolver decision should expose these fields in command signals and a
small decision artifact:

```text
actionResolver.version
actionResolver.target.query
actionResolver.primaryMethod
actionResolver.selectedMethod
actionResolver.fallbackAllowed
actionResolver.fallbackUsed
actionResolver.fallbackReason
actionResolver.policy
actionResolver.cursorDisturbance
actionResolver.pressMechanism
```

For `smartPress`, the initial values are:

| Field | AX path | Pointer fallback path |
| --- | --- | --- |
| primary method | `ax-action` | `ax-action` |
| selected method | `ax-action` | `pointer-click` |
| fallback allowed | input `allow_pointer_fallback` | input `allow_pointer_fallback` |
| fallback used | `false` | `true` |
| fallback reason | `none` | primary AX error |
| cursor disturbance | `none` | `warp-visible` |
| press mechanism | `ax-action` | `pointer-click` |

## Non-goals

- Do not claim true no-steal coordinate click.
- Do not add SkyLight or private macOS SPI.
- Do not make `debug.smartPress` valid for production validated cases.
- Do not replace the recipe system with a new orchestration language.
- Do not add YOLO or realtime tracking to this layer.

## Relationship to existing contracts

`SurfaceSelector` and `RecognitionResult` produce grounded candidates.
`ActionResolver` consumes those grounded targets and chooses a control method.
`VerificationResult` still proves whether the expected state happened.

The stable direction remains:

```text
surface observation -> candidate / node / recognition
  -> action resolver decision
  -> action dispatch
  -> verification result
```

## Next useful extensions

1. Let `music.result.play` report its row activation path through the same
   resolver fields instead of preserving only `smartPress.*` compatibility
   signals.
2. Add an `ax-only` policy to recipe-level action steps so production skills
   can explicitly deny pointer fallback.
3. Teach overlay evidence annotations to show the resolver decision alongside
   the OCR/AX/row target.
4. Add `candidate_ref` / `node_ref` inputs once action targets consistently
   come from structured observation artifacts.
