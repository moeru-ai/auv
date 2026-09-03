# Click point and typed target contract

Status: implemented frontend and typed invoke contract.

## Contract

`auv invoke input.clickPoint X Y` replaces the two invoke command ids
`input.clickScreenPoint` and `input.clickWindowPoint`.

The shared `ExecutionTarget` is one of:

- `Application { id }`, spelled `--target app:<bundle-id>`;
- `Window { id }`, spelled `--target window:<window-id>`;
- `Display { id }`, spelled `--target display:<display-id>`.

Bare target values remain application ids for existing callers. MCP exposes the
same alternatives as the mutually exclusive `application_id`, `window_id`, and
`display_id` fields.

`--relative-to screen|window|display` selects the coordinate basis. When it is
omitted, no target selects `screen`, application/window selects `window`, and
display selects `display`. `--normalized` is available only for window and
display coordinates.

The accepted target/basis combinations are:

| Target | Basis | Delivery |
|---|---|---|
| none | screen | existing global screen-point click |
| application | window | resolve the application's window, then existing window click |
| window | window | bind the listed window id, then existing window click |
| display | display | project display-local coordinates to screen, then existing global click |

Other combinations fail before input delivery. `--input-policy` is valid only
for a window-relative click, and `--title` is valid only with an application
target.

## Ownership

`auv-cli-invoke` owns parsing, combination validation, projection, direct
results, and local/selected-Runner dispatch. The root CLI and MCP frontend carry
the same typed target. The `auv` Runner client can bind a window returned by
`list` so a window-id target retains its `WindowRef` route.

No `auv-driver*` contract or implementation changed. The unified command maps
to the existing global and window-targeted input capabilities and keeps the
driver-owned `InputActionResult` unchanged.

## Deferred

Target-aware `input.key`, `input.typeText`, and `input.pasteText` remain a
separate slice. Their current call sites retain the typed input-target activation
TODO because this click-point contract does not define a keyboard focus lease.
