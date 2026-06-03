# AUV View Parser Example CLI Rendering v0

Date: 2026-05-29

Status: v0 CLI rendering spec. Pins what `netease-playlist-ls` (and
any future view parser example binary) prints, in which mode, with
what exit codes.

Audience: owner, reviewers, and any agent (Codex, Claude, others)
writing the example CLI binary or scripts that consume its output.

## Purpose

The design doc says:

> Structured output is the primary contract. Human CLI text is a
> renderer over structured artifacts.

It does not pin what the renderer must produce, what is required vs
optional in the human output, or how diagnostics surface. Without
this, two CLI implementations land at different formats and downstream
scripts cannot parse either reliably.

This spec pins the v0 CLI contract.

## Relationship to other specs

```text
view-parser-ir-shapes-v0.md            artifact JSON shapes
view-parser-diagnostic-policy-v0.md     diagnostic outcomes
view-parser-trace-layout-v0.md          root-span signals
view-parser-example-placement-v0.md     binary lives in the example crate
view-parser-layer-contracts-v0.md       composition that produces the artifacts
view-parser-cli-rendering-v0.md  (this) CLI rendering + exit codes
```

## Operating principle

The CLI never computes new facts. Every line it prints is read from
the `ViewReconstruction`, the `ViewProjection`, the `ParserDiagnostic`s,
or the four root-span signals. If a number or label is not in those
sources, it does not appear in the output.

This makes the structured artifacts authoritative. A reviewer reading
the artifact JSON sees the same outcome the CLI rendered.

## Output modes

| Mode | Flag | Audience | Shape |
|---|---|---|---|
| Default (human) | (none) | terminal reader | sectioned text |
| JSON | `--json` | scripts, agents | one JSON document = the full `ViewProjection` record |
| Verbose | `--verbose` | debugger | default text + per-observation summary + diagnostic table |

Modes are mutually exclusive. `--json --verbose` errors before the
parse run starts. Combined modes are explicitly out of scope for v0.

## Default (human) output

The default shape is:

```text
NetEase Playlist Sidebar (run <run_id>, <observation_count> observations)

Sections:
  My Playlists (12 items)
    1. Liked Songs
    2. Daily Mix 1
    …

  Recommended (4 items)
    1. Discover Weekly
    …

Outcome: <clean | observed-failure | infra-failure>
Known limits:
  - <one per known_limits entry, when present>
```

Required lines:

- The header (one line): run id and observation count come from the
  root-span signals (`view.parse.scope_id`, `view.parse.observation_
  count`).
- `Sections:` block: one section per `SidebarSection` in the
  projection. Items in declared order. Item numbering restarts per
  section.
- `Outcome:` line: one of the three values from the
  `view.parse.outcome` signal.
- `Known limits:` block: appears only if the reconstruction has at
  least one entry. Each entry is one bullet.

Forbidden in default output:

- Per-observation breakdowns (those are verbose).
- Diagnostic kind dumps (those are verbose; only the count and the
  Fatal kind name surface in default).
- Bounds, fingerprints, IDs (those are machine concerns).
- ANSI color when not writing to a TTY.

If `view.parse.outcome` is `observed-failure`, the `Outcome:` line
includes the Fatal diagnostic kind:

```text
Outcome: observed-failure (RegionCollapsed)
```

If it is `infra-failure`, the CLI prints the bubbled `Err(...)`
message on stderr and exits non-zero before reaching the `Outcome:`
line. See exit codes below.

## `--json` output

When `--json` is set, the CLI writes exactly one JSON document to
stdout:

```text
{
  "schema_version": "view-projection-v0",
  "projection": <ViewProjection<P> serialized per ir-shapes-v0>,
  "reconstruction_ref": <ArtifactRef pointing at the view-reconstruction artifact>,
  "outcome": "clean" | "observed-failure" | "infra-failure",
  "fatal_diagnostic_kind": <ParserDiagnosticKind name | null>,
  "run_id": <RunId>,
  "observation_count": <usize>
}
```

This wrapper is **not** itself a new artifact — it is a CLI-output
record only. The structured artifact (`view-projection-<domain>`)
remains the source of truth in run storage. The CLI record merely
collects the four root-span signals next to the projection for
convenience.

Forbidden in `--json` output:

- Mixed text and JSON.
- Trailing log lines on stdout. All non-JSON output goes to stderr.
- Color escapes.

## `--verbose` output

`--verbose` extends the default human output with:

- A per-observation table after `Sections:`:

  ```text
  Observations:
    obs[0]  viewport=… fingerprint=…  candidates=N
    obs[1]  …
  ```

- A diagnostic table after `Outcome:`:

  ```text
  Diagnostics:
    <kind>  obs=<index>  node=<short id>  message=<one line>
    …
  ```

The diagnostic table lists every entry from
`ViewReconstruction.diagnostics`, sorted by `kind` then by
`observation_index`. Multi-line messages collapse onto one line in
verbose; full messages remain in the JSON artifact.

`--verbose` does not change exit codes or success semantics; it only
expands what is printed.

## Diagnostic display rules

| Mode | What surfaces |
|---|---|
| Default | Outcome line; if Fatal, the Fatal kind name in parentheses; total diagnostic count only when ≥ 1 |
| `--json` | Full diagnostics carried inside the projection record; reader gets everything |
| `--verbose` | Default content + the diagnostic table |

Reader-side severity comes from the diagnostic policy spec; the CLI
does not store or compute severity.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Parse run produced a reconstruction (`view.parse.outcome = "clean"`) |
| 1 | Parse run completed but reported observed failure (`view.parse.outcome = "observed-failure"`) |
| 2 | Infrastructure failure (`view.parse.outcome = "infra-failure"` or the run bubbled `Err(...)`); no usable reconstruction |
| 64 | CLI invocation error (bad flags, mutually-exclusive modes, unknown subcommand) — modeled after `sysexits.h EX_USAGE` |

Exit code 1 distinguishing observed from infra is critical: scripts
that retry on infra failure but surface observed failure to a human
need to be able to tell them apart without parsing stdout.

## Stream discipline

- Default mode: human text on stdout. Errors on stderr.
- `--json`: JSON document on stdout, **only** the JSON document, with
  a trailing newline. All informational logging goes to stderr.
- `--verbose`: human text on stdout including the verbose blocks.
  Errors on stderr.

No mode writes anything to stdout before the parse run completes (no
progress indicators). If progress is needed later, it goes to stderr
and is suppressed when stderr is not a TTY.

## Localization

v0 default output is English only. Item labels, section names, and
diagnostic messages may contain non-English characters from the
underlying data (e.g. Chinese song titles); the renderer does not
transliterate.

`--json` output is locale-neutral. Numbers are decimal, dates (if
any) are RFC 3339.

## v0 done criteria

The CLI rendering is v0-complete when:

1. The binary supports default, `--json`, and `--verbose` modes
   exactly per the tables above.
2. Default human output passes a snapshot test for each of:
   clean / observed-failure / infra-failure outcomes.
3. `--json` output validates against the wrapper schema above and
   contains a complete `ViewProjection<P>` payload.
4. `--verbose` output extends default with both the observation table
   and the diagnostic table; their content is read from the
   reconstruction and projection only.
5. Exit codes match the table above; a test exercises 0 / 1 / 2 / 64
   each.
6. Stream discipline: a smoke test confirms `--json` writes nothing
   to stdout other than the JSON document.
7. The renderer module (`projection::render`) reads only from
   `ViewProjection<P>`, `ViewReconstruction`, and root-span signals.
   Adding a new dependency to it (e.g. live driver state) fails
   review.

## Forbidden in v0

- Computing new facts in the renderer. Every printed value reads from
  the artifacts or signals.
- Mixing modes (e.g. `--json --verbose`).
- Color codes when stdout is not a TTY (must auto-detect).
- Adding subcommands. v0 is a single binary with the three modes;
  subcommands wait for a real need.
- Progress indicators on stdout. Use stderr.
- Hard-coding NetEase strings in the renderer. Strings come from the
  projection / known_limits / signals only. (The binary name is
  permitted; the header text is not — it is derived from projection
  domain.)

## Non-goals for this spec

Intentionally deferred:

- Pagination / pager integration.
- Interactive output (item selection, scroll).
- Localized human output. v0 is English text + raw labels.
- Output for non-NetEase examples. v0 covers `netease-playlist-ls`
  only; a second example may revise these defaults.
- Configurable templates.
- Streaming output during the parse run.
- Machine output formats other than JSON (no TSV / YAML / NDJSON).

## How to use this spec

When writing or reviewing the renderer:

- Start from the default-mode shape above. Add fields only when the
  spec table says they should appear.
- If a desired output value is not in `ViewProjection`,
  `ViewReconstruction`, or root-span signals, the renderer must not
  print it — file a gap to add it to the artifact instead.
- Exit code 1 vs 2 is the script-level signal that matters most. Get
  it right before iterating on text formatting.

This document is part of the convergence phase. Revisions are explicit,
dated, and owner-approved.
