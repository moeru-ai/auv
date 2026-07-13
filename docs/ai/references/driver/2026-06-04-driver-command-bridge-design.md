# AUV Driver Command Bridge Design

Status: approved design for implementation planning
Date: 2026-06-04

## Goal

Migrate the existing command, recipe, skill, runtime, and bundle execution
chain toward `auv-driver` as the only atomic automation capability layer.

The first implementation phase must not invent a new primitive API. It should
map existing legacy command handlers to `auv-driver` capabilities, identify
missing `auv-driver` surfaces, add those surfaces where needed, and then
replace or delete legacy recipe and bundle paths once their useful capability
coverage is accounted for.

## Current Shape

The current legacy path is:

```text
cli invoke / recipe / skill
  -> CommandCatalog
  -> Runtime::invoke
  -> DriverCall { operation: string }
  -> macOS legacy handler
  -> DriverResponse / artifacts / trace
```

This path still provides useful migration evidence because it enumerates the
commands and workflows AUV has historically exposed. It should be treated as a
compatibility harness during migration, not as the long-term execution model.

The target path is:

```text
CLI invoke / domain crate / future MCP
  -> auv-driver typed API
  -> auv-driver-macos implementation
  -> recording / artifacts / inspect
```

`auv-driver` is the atomic capability layer. There should be no new
`src/primitives` layer or parallel primitive API between command handlers and
`auv-driver`.

## Scope

The design covers these areas:

- `src/catalog.rs`
- `src/runtime.rs`
- `src/cli.rs` legacy `invoke`
- `src/driver/macos/**` legacy command handlers
- `src/skill/**`
- `src/bundle/**`
- `recipes/**`
- `bundles/**`
- `crates/auv-driver/**`
- `crates/auv-driver-macos/**`

The design does not cover:

- MCP implementation
- NetEase domain crate redesign
- inspect viewer UI redesign
- broad cleanup unrelated to the command-to-driver migration

## Migration Strategy

Use capability buckets instead of CLI command names as the organizing unit.
For each existing command, record which capability it needs, whether
`auv-driver` already exposes that capability, and whether
`auv-driver-macos` implements it.

The migration matrix should classify every command as one of:

- `migrate`: keep the legacy command temporarily, but implement the handler by
  calling `auv-driver` / `auv-driver-macos` typed APIs.
- `already-bridged`: the handler already routes through typed driver APIs
  enough for this phase.
- `driver-gap`: the command exposes a useful capability that is missing from
  `auv-driver`; add the capability to `auv-driver`, then implement it in
  `auv-driver-macos`.
- `delete`: the command or recipe path is historical and should not be
  preserved.
- `defer`: the capability is valid but outside the first bridge phase; leave a
  `TODO:` or `NOTICE:` marker at the relevant call site before moving on.

## Capability Buckets

### Window

Includes listing windows, resolving application/window selectors, returning
stable window references, and exposing enough metadata for downstream capture,
input, and reconstruction.

Expected migration outcome:

- Legacy `list_windows` and window resolution helpers call typed
  `auv-driver` window/session APIs.
- Missing selector metadata belongs in `auv-driver`, not in one-off root-crate
  helpers.

### Capture

Includes display, region, and window capture, image artifact production, and
coordinate metadata.

Expected migration outcome:

- Capture operations use typed capture APIs where possible.
- Coordinate metadata remains preserved for artifacts and inspect.
- If current capture contract types are root-crate only, the implementation
  plan must decide whether to move shared capture concepts into
  `auv-driver` or leave them as artifact-layer records outside the driver.

### Input

Includes click, scroll, type, paste, key press, activation policy, foreground
policy, fallback selection, and `InputActionResult`.

Expected migration outcome:

- Legacy handlers map input outcomes to `InputActionResult`.
- Driver results preserve selected path, attempts, fallback reason, and
  disturbance metadata.
- Command handlers stop owning input-delivery policy when the same policy
  belongs in `auv-driver`.

### AX

Includes AX tree capture, node lookup, press, focus, and text-bearing node
verification.

Expected migration outcome:

- AX behavior that can be made reusable goes into `auv-driver` traits/types and
  `auv-driver-macos` implementation.
- Legacy handlers only parse compatibility inputs and render compatibility
  `DriverResponse`s.

### Vision

Includes OCR, row detection, image matching, icon matching, and recognition
results.

Expected migration outcome:

- Vision capability boundaries are explicit in `auv-driver`.
- Existing recognition / candidates / action resolver seam is preserved.
- Command handlers do not mint a second recognition schema.

### Permission And Session

Includes permission probing, local session creation, foreground activation, and
host platform readiness.

Expected migration outcome:

- Permission and session capabilities live behind driver APIs.
- macOS-specific implementation remains in `auv-driver-macos`.
- Root crate code does not depend on `auv-driver-macos` unless it is an
  explicitly macOS-gated adapter.

## Recipe, Skill, And Bundle Policy

`src/skill`, `src/bundle`, `recipes`, and `bundles` are not long-term
architecture surfaces for this migration.

During the bridge phase they may be used only as:

- migration evidence for what legacy workflows required
- temporary regression harnesses while command handlers are replaced

They should not receive new core behavior. Once their useful capability
coverage is represented by `auv-driver` or explicitly marked as deleted, they
may be removed through a breaking migration.

NetEase-specific legacy recipes do not need to be preserved because the current
NetEase implementation lives in `crates/auv-netease-music`.

## Runtime And Recording Policy

`Runtime::invoke(CommandSpec)` and `CommandCatalog` are compatibility surfaces
during migration. They should not be expanded into the future execution model.

Run recording, artifact persistence, trace records, `OperationResult`,
`VerificationResult`, and inspect read-side behavior should be preserved and
eventually made usable by typed `auv-driver` callers and domain crates.

The implementation plan should separate:

- old command routing, which can be removed
- recording and artifact infrastructure, which should survive

## CLI Invoke Policy

The old CLI invoke shape:

```text
auv-cli invoke <command-id> --key value
```

may remain temporarily as a compatibility harness.

The future CLI invoke should be shaped around `auv-driver` capabilities rather
than command catalog IDs, for example:

```text
auv-cli invoke window list
auv-cli invoke capture window --target <bundle-id>
auv-cli invoke input scroll-window-region --target <bundle-id> --region <l,t,r,b> --dy <n>
```

The first bridge phase does not have to implement the future CLI shape. It must
avoid making the old shape harder to delete.

## Validation Strategy

Every migration batch should run:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
```

While legacy command catalog remains present, command changes should also run:

```bash
cargo run --quiet -- list-commands
```

After a new `invoke` capability surface exists, add a smoke command for at
least one read-only capability such as window listing.

## Exit Criteria For First Implementation Plan

The first implementation plan is complete when it includes:

- a command-to-capability migration matrix
- a list of `auv-driver` capability gaps
- a first batch of handler migrations grouped by capability
- tests or smoke checks for migrated handlers
- a deletion plan for legacy skill, recipe, and bundle paths
- explicit deferral markers for valid capabilities not migrated in the first
  batch

