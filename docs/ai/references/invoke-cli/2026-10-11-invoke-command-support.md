# Invoke command support

Status: implemented. This is the "Support information" slice of
[#211](https://github.com/moeru-ai/auv/issues/211). The term is provisional; see
[Command Support](../../../TERMS_AND_CONCEPTS.md#command-support).

## Problem

The help index listed every registered command the same way on every host, so a command with
no path on the current platform looked as available as one that works. The only platform
information was prose in some descriptions, such as "(macOS, Linux and Windows)", which nothing
checked. The checklist in #211 held the real answer, but it is maintained by hand and had already
drifted from the source.

## What a cell means

Each command declares one cell per platform, in `invoke_command`:

```rust
support = { macos: Supported, linux: Partial("foreground delivery only"), windows: Unsupported },
role = Alias("input.keys"),   // optional; Fixture for data-only commands
```

| State | Meaning |
| --- | --- |
| `supported` | A direct, non-dry-run local path exists for everything the command accepts. |
| `partial` | A path exists for some inputs or delivery modes. The note names the limit. A `Partial` without a note does not compile. |
| `unsupported` | No path. The command refuses, or completes without doing the work and says so. |
| `not_evidenced` | Nobody has audited the path. No claim either way. |

A cell is a statement about source, matching the rule #211 uses for its checkboxes. It is not
behavior evidence: `supported` does not say the path was run. A `--dry-run` that succeeds does not
make a cell supported, and command registration is not support. Evidence levels stay in
[the support matrix](../../../SUPPORT_MATRIX.md).

`not_evidenced` is the honest state for a path nobody has read. No cell uses it yet, because every
cell below was read from source. It exists so that a future contributor who cannot audit a platform
can say so instead of guessing.

## Where it is declared and shown

- **Declaration.** `support` is a required `invoke_command` attribute naming all three platforms, so a
  new command that omits one does not compile. `typed_spec` takes the cells directly, so there is
  no other way to build a command.
- **Registry.** `InvokeRegistry::from_groups` checks that an alias names a registered command that is
  not itself an alias, and declares the same cells as it. The registry tests pin the two aliases
  and two fixture commands, so a third is a decision.
- **Help.** `auv invoke --help` shows `yes`, `partial`, `no` or `?` per platform beside each command.
  `auv invoke <COMMAND> --help` adds a `Platform support:` section with each partial cell's note.
- **MCP.** `x-auv-commands[].support` and `.role` carry the same declaration. A test compares the
  metadata with the registry for every command, so the two frontends cannot disagree.

## What the first table found

The cells were read from `cfg` gates in the command modules and from the platform drivers'
explicit unsupported branches, on 2026-10-11. The
[coverage audit](2026-10-11-invoke-platform-coverage-audit.md) holds the counts, the comparison
with the #211 checklist, and the Windows gaps. What shapes the mechanism:

- The checklist was right when it was written, with 37 command IDs, and fell behind. The registry
  has 42 now. Nothing ties a hand-kept checklist to the registry, which is the problem the typed
  cells solve for the declaration itself.
- Several Windows commands have a driver capability behind them that the command does not use
  (`mediaControl.*`, `input.pasteText`, `app.activate`, `overlay.*`). The cell's note says so
  where it does, so a reader of `auv invoke <COMMAND> --help` can tell "nothing exists" from "not
  connected".

Most `partial` cells are input commands. On Linux and Windows the keyboard commands deliver to the
foreground only and reject a target. On Linux the window-bound pointer commands focus the window
and use foreground portal input. On Windows they post window messages, so whether an application
acts on them depends on its toolkit (see the
[background session reference](../driver/2026-10-10-windows-background-session.md)).

## Evidence and limits

- **Tested.** The registry, help and MCP tests run on each CI platform. They check that the
  declaration is complete, internally consistent, and rendered the same by both frontends.
- **Not validated.** Nothing checks that a cell matches the code it describes. A `cfg` gate and a
  cell can drift apart, and a reviewer has to catch it. No test runs a command on a platform to
  confirm its cell, because a dry run proves nothing about support and a live run drives a desktop.
  The macOS and Linux cells were read, not run, in this slice.
- **Not covered.** A selected Runner's support is a separate route. `TODO(invoke-support-runner-route)`:
  a second set of cells, after the Runner audit #211 asks for. The
  [Runner route proposal](2026-10-13-runner-route-support-design.md) records where the route
  differs from these cells today.
- **Not generated.** No cell claims more than #211's own status rules allow. The #211 checklist is
  edited by hand from the [coverage audit](2026-10-11-invoke-platform-coverage-audit.md); nothing
  produces it from the registry, so it can fall behind again. The registry is the live answer.
