# Apple Music platform integration layout

Date: 2026-07-25

Status: accepted app-crate layout and alint convention

## Decision

Apple Music keeps app automation and other platform-owned integration code in
`src/platforms/`. Each file owns one cohesive capability on one platform and
uses the `<capability>_<platform>.rs` form:

```text
src/platforms/
  launch_windows.rs
  playback_windows.rs
  probe_macos.rs
  search_windows.rs
  transport_windows.rs
  window_windows.rs
```

`platforms` is the boundary name because it also covers observation-only
probes and future platform-specific view parser acquisition. `actions` would
incorrectly imply that every module delivers input, while `interaction`
already names the orchestration layer above drivers in AUV's shared
vocabulary.

The capability file owns driver setup, platform UI interpretation, input
delivery, platform fallback policy, and platform verification for that flow.
It should not be split into one file or wrapper per driver call. CLI parsing
and presentation remain outside this directory. Platform-neutral contracts
and reusable parser IR may also remain outside it.

Apple Music does not currently have an approved view parser slice. If one is
added, its platform acquisition and adapters belong in a capability/platform
file under `src/platforms/`; this decision does not authorize implementing the
parser itself.

## Enforcement

The alint rule `require-platform-scoped-app-integration` reviews an app crate
as a directory so it can identify misplaced automation, unclear platform file
ownership, scattered wrappers, and platform-specific view parser code mixed
into neutral parser modules. Its review prompt states the architectural
conventions directly and uses filesystem inspection plus semantic judgment.

The rule is enabled for the current desktop app integration crates in
`js/packages/alint-config/src/config.ts`, including Apple Music, Apple Notes,
Apple TextEdit, NetEase Music, QQMusic, and GNOME Control Center.
