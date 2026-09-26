# AUV base evaluations

Opt-in behavior evaluations against real application receivers. Rust owns task
execution, process lifetimes, receipt collection, cases, and assertions. Electron
and browser test objects are authored in TypeScript. AppKit objects and native
macOS focus operations use Swift. **Python is not required.**

## Layout

```text
platforms/
  desktop-universal/
    test-objects/          Shared Electron and browser receivers (TypeScript)
  desktop-macos/
    test-objects/appkit/   Swift text receivers and the foreground anchor
    tasks/                Rust keyboard and focus evaluation runners
      native/             Swift focus controllers
    cases/                Rust scenarios, assertions, and AppKit evaluation
  desktop-windows/        Reserved; no implemented evaluations yet
  desktop-linux/          Reserved; no implemented evaluations yet
tasks/                    Shared Rust process IO and HTTP receipt protocol
justfile                  Build, check, and run recipes exposed as just eval
results/                  Ignored local output
```

Test objects observe what the receiving application processes. Tasks own launch,
input dispatch, sampling, and cleanup. Cases define inputs and acceptance checks.
The structure follows vieval's task/case distinction; it does not depend on the
vieval runtime or a model API. Platform directories can later include mobile
suites without moving the existing desktop suites. Shared desktop objects alone
are not a claim of validated Windows/Linux behavior.

The no-input authentication contract test remains in `auv-driver-macos/tests`:
it validates native event preparation without launching a GUI receiver.

## Test object scope and naming

Test object directories describe the application technology: `appkit/`,
`electron/`, and `web/`. Keep these names as the suite grows. An application
can host multiple test areas, with capability-specific source files such as
`keyboard.ts` or `keyboard.swift` for keyboard input and `focus.ts` or
`focus.swift` for focus observation.

The current objects cover keyboard input, text editing, and focus observation.
The keyboard and focus tasks share the web keyboard page; the capability names
do not require a separate application for every task.

When adding an approved mouse, scroll, or drag evaluation, extend the appropriate
test application with a dedicated area or module. Keep input sequences and
acceptance checks in the platform's `tasks/` and `cases/`; test objects provide
the receiving UI and observations. Keep existing `keyboard.*` modules scoped to
keyboard behavior.

NOTICE: Mouse, scroll, and drag test areas are not implemented in this slice.
Add them when an approved evaluation needs them; the general application names
do not imply coverage of those capabilities.

## Setup and build

Use `just` 1.33 or later (`nix develop` includes it). From the repository root:

```sh
just eval
just eval setup
just eval build
just eval check
```

Recipes live in this suite's `justfile` and run from the repository root.
Just owns command orchestration; Rust owns case execution, assertions, and
receiver cleanup. `check` does not launch applications.

Each macOS task has its own Rust entry point: `keyboard-eval` and `focus-eval`.
Just calls these directly; there is no additional platform/subcommand dispatcher.
The shared library provides receiver IO and cancellation. macOS options, native
fixture setup, and case assertions live under `platforms/desktop-macos/`.

The Rust tasks require macOS, Xcode Command Line Tools (`swiftc`), Accessibility
access, and the selected target applications. Supply an **installed Electron
executable** in `$electron_binary`; the package above supplies Electron's types
and does not install its app binary with `--ignore-scripts`. Chrome defaults to
its standard `/Applications` location and can be overridden with `--chrome`.
Chrome focus evaluation additionally requires `agent-browser` on PATH. It reads
DOM state and screenshots; AUV supplies the input under test.

Node/pnpm build the TypeScript receivers. Generated JavaScript is ignored in
`desktop-universal/test-objects/dist/`; no hand-maintained JS or Python runner is
needed. Rust compilation does not invoke Node, pnpm, or Python.

The test-object package uses `tsc --noEmit` for type checking. Both TypeScript
configs extend the root `tsconfig.json`. The main config supplies Node types for
Electron and the build configs; `tsconfig.web.json` supplies DOM types without
Node globals.

`electron-vite` builds the Electron main-process entries into
`dist/electron/*.js`. It owns Electron module externalization and runtime
targets. These receivers have no preload or local renderer build. During a run,
Vite serves the shared TypeScript page to Electron and Chrome. Its local proxy
forwards `/command` and `/receipt` to the Rust receipt API. Rust no longer
serves HTML or compiled JavaScript.

## Run

```sh
# Complete key presses through the CLI.
just eval keyboard \
  --electron "$electron_binary" \
  --output evals/auv-base/results/cli-background --repetitions 5

# Holds through the persistent public Rust driver caller.
just eval hold \
  --electron "$electron_binary" \
  --output evals/auv-base/results/held-background --repetitions 5

# No-raise/key-window/restoration experiment.
just eval focus \
  --electron "$electron_binary" \
  --output evals/auv-base/results/no-raise-key \
  --mode no_raise_key --restore records --require-success --repetitions 3

just eval appkit
```

The `keyboard`, `hold`, and `focus` recipes build their inputs before running.
Additional arguments are passed directly to the Rust task. To inspect its CLI
without building, use `just --no-deps eval keyboard --help` after the first build.

Choose a fresh output directory for each run. Receipts, screenshots, profiles,
and compiled test objects stay in ignored `results/` or a caller-supplied local
temporary directory. They are excluded from PRs.

For keyboard foreground controls, add `--mode foreground`. Repeat `--case` to
select scenarios, for example `--case held_shift_press_b` with `--sender`.
For the focus experiment, repeat `--mode` to compare `baseline`, `no_raise`,
`no_raise_key`, `no_raise_click`, and `foreground`. `--receiver` restricts the
selected apps. Both tasks expose their options through `--help`.

## Result interpretation

- Keyboard exits 1 when any case fails, including known background select-all
  and emoji limitations. Invalid foreground/input-source preconditions are
  reported separately from valid receiver failures.
- Focus normally collects diagnostic controls, including expected failures, and
  exits 0 when collection completes. `--require-success` additionally requires
  text, focus restoration, and the selected delivery posture checks.
- Native submission is not semantic verification. Receivers check text, event
  counts, timing, and focus separately from `InputActionResult`.
- No-raise preparation remains test-only. Internal focus temporarily transfers
  even when sampled foreground identity and window order stay unchanged. These
  evaluations do not establish safe concurrent typing, crash recovery, or support
  on other macOS versions, Spaces, minimized windows, or arbitrary app layouts.

## Check the Rust migration against local receipts

The optional regression test replays 186 keyboard and 108 focus receipts from
previous local runs and compares every check, including failures. The inputs
remain outside Git:

```sh
just eval replay "$local_keyboard_validation_directory"
```

The [keyboard contract](../../docs/ai/references/driver/2026-09-24-keyboard-hold-contract.md)
and [focus experiment](../../docs/ai/references/driver/2026-09-26-no-raise-keyboard-probe.md)
record previous local results and their limits.

Local migration validation (2026-09-26, macOS 26.3): all 294 recorded receipts
produced identical checks. A fresh bounded run passed 9/9 focus cases across
Swift/Electron/Chrome, 8/8 held-key cases across Chrome/Electron, and 2/2 CLI BMP
text cases. Rust build/Clippy, TypeScript compilation, and lint passed. Raw output
remains local; these samples do not expand the platform support claims above.

PR worktree recheck (2026-09-27): Chrome background `A猫` passed 1/1. Electron
background `A猫` passed 4/6. Both failures inserted the expected text with the
expected focus state, but the final `keyup` was absent from both Electron's
native event log and the DOM receipt. The key-pair assertion remains strict;
this intermittent result is still open. Raw receipts remain local.
