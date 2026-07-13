# macOS Driver Legacy and Typed Path Handoff

Date: 2026-05-26

Status: current handoff for the next coding agent

Current HEAD when written: `8546479`

## Start Here

Read these files first, in this order:

1. `AGENTS.md`
2. `docs/TERMS_AND_CONCEPTS.md`
3. `docs/ai/references/driver/2026-05-25-driver-platform-api-crates-design.md`
4. `src/driver/mod.rs`
5. `src/driver/macos/mod.rs`
6. `src/driver/macos/dispatch.rs`
7. `src/driver/macos/typed.rs`
8. `crates/auv-driver-macos/src/driver.rs`
9. `crates/auv-driver-macos/src/session.rs`
10. `examples/netease_play_visible_anchor.rs`

The important repo fact is simple:

- the old root command path is still alive
- it is still the default runtime and catalog path
- the typed crates exist and are already useful
- the current work is about tightening the boundary between them, not pretending the migration is finished

## Current Repo State

When this handoff was written, the local checkout was:

```text
branch: main
status: clean
remote relation: ahead of origin/main by 3 commits
```

Recent commits that matter:

```text
8546479 refactor(macos): centralize typed compatibility shims
4601c5d refactor(macos): clarify legacy command adapter
4d5a220 feat(verify): emit typed OperationResult artifact from verify.axText
1dd857e feat(inspect): expose stored verifications and observations
500e8c7 feat(runtime): add child spans for recorded typed operations
c7764b6 feat(contract): promote verification to a first-class OperationResult field
```

Before coding, verify the live state:

```bash
git status --short --branch
git log --oneline --decorate -6
```

## What Is True Right Now

### Default runtime path

The default driver registry still registers the root legacy macOS adapter:

- `src/driver/mod.rs`
- `LegacyMacosCommandDriver`

The command catalog still routes the macOS command surface through
`driver_id = "macos.desktop"`:

- `src/catalog.rs`

The old command-facing dispatch is still the primary command execution surface:

- `src/driver/macos/dispatch.rs`

That dispatch still owns the current command families:

- capture
- observe
- control
- overlay

### Typed driver path

The typed platform path now exists in separate crates:

- `crates/auv-driver`
- `crates/auv-driver-macos`
- `crates/auv-overlay-macos`

The typed macOS session API is already real enough to back examples and narrow
adapters:

- `crates/auv-driver-macos/src/driver.rs`
- `crates/auv-driver-macos/src/session.rs`
- `examples/netease_play_visible_anchor.rs`

### Legacy and typed boundary

The root macOS module is now explicitly framed as a legacy command adapter, not
as the long-term center of platform implementation:

- `src/driver/macos/mod.rs`

This handoff session added a local compatibility shim:

- `src/driver/macos/typed.rs`

That shim centralizes the most obvious root-to-typed borrowing points:

- legacy descriptor metadata
- observe helper re-exports
- narrow typed session paste-text bridge

This means the old command handlers do not need to keep opening typed sessions
or importing typed observe helpers directly inside random command files.

## What Landed In This Local Stack

### `1dd857e feat(inspect): expose stored verifications and observations`

Read-side inspect flow can now surface stored verification and observation data
from recorded artifacts.

### `4d5a220 feat(verify): emit typed OperationResult artifact from verify.axText`

`verify.axText` now writes a typed `OperationResult` artifact instead of being
only a summary/signals-only verification path. This ties the verify command
more cleanly into the structured contract line.

Relevant file:

- `src/driver/macos/observe.rs`

### `4601c5d refactor(macos): clarify legacy command adapter`

The old root driver was renamed from `MacOsDesktopDriver` to
`LegacyMacosCommandDriver`, and the command dispatch was split by operation
family instead of remaining one flat oversized match.

Relevant files:

- `src/driver/macos/mod.rs`
- `src/driver/macos/dispatch.rs`

### `8546479 refactor(macos): centralize typed compatibility shims`

This commit tightened the old/new boundary one step further by moving the
obvious typed borrowing points behind `src/driver/macos/typed.rs`.

Concrete effects:

- `src/driver/macos/control/text.rs` no longer opens typed sessions directly
- `src/driver/macos/observe.rs` no longer imports typed observe helpers directly
- `src/driver/macos/descriptor.rs` now resolves typed legacy metadata through
  the local compat shim

## Validation Already Run

At `8546479`, the following commands passed:

```bash
cargo fmt --check
cargo check
cargo test
git diff --check
cargo run --quiet -- list-commands
cargo run --quiet -- skill cases list
```

The test result was:

```text
379 lib tests passed
23 cli tests passed
```

## Good Continuation Options

The repo is in a real dual-path transition, but not in a "two equal production
paths" state. It is more accurate to think of it like this:

```text
primary path:
  catalog -> LegacyMacosCommandDriver -> legacy command dispatch

emerging path:
  typed crates -> typed session APIs -> narrow root adapters and recorded typed operations
```

From here, a next agent can continue along any of these lines without needing
to reopen the overall framing:

1. Continue moving direct root references to `auv-driver-macos::*` behind
   `src/driver/macos/typed.rs` or another explicit local compat module.
2. Move one more narrow legacy command to a typed-session-backed adapter where
   the semantics already fit cleanly.
3. Expand the recorded typed operation path so typed driver flows and runtime
   recording meet more naturally.
4. Keep improving read-side consumption of verification and observation
   contracts in inspect and viewer surfaces.

## Places That Still Show Transition Seams

If the next agent wants obvious places where the old/new split is still visible,
start here:

- `src/driver/macos/support/geometry.rs`
- `src/driver/macos/support/mod.rs`
- `src/driver/macos/constants.rs`
- `src/driver/macos/capture/mod.rs`
- `src/driver/macos/native/mod.rs`

Those files still expose compatibility re-exports from `auv-driver-macos` into
the root crate. That is not inherently wrong, but it is where the migration
seams are still most obvious.

## Current Mental Model

If a future change needs a quick sanity check, use this:

```text
old path:
  owns command-facing compatibility
  still owns the default registry/catalog execution path
  should become thinner over time

typed path:
  owns platform APIs and platform facts
  should grow as the real implementation center
  should not be forced back into string-command semantics internally
```

The current repository already reflects that split in code shape, just not yet
in final execution ownership.
