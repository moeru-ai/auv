# AUV Qodana Operating Model

Durable reference for how AUV consumes Qodana across observatory (full scan),
PR gate (blocking), and debt research. Canonical config lives in
[`qodana.yaml`](../../../qodana.yaml) and [`.qodana/profiles/`](../../../.qodana/profiles/).

## Problem

A single `qodana.yaml` that sets `profile.base: empty` plus a hand-picked
inspection list deletes the factory observation surface and treats Qodana as a
narrow hygiene linter. AUV is a STEM / multi-crate monorepo; we need a wide
quality map with layered consumption, not profile amputation to quiet CI.

## Three consumption layers

| Layer | Profile | CI job | Branch protection | Purpose |
|-------|---------|--------|-------------------|---------|
| Observatory | `qodana.recommended` | `qodana-observatory` | No | Full signal, Cloud trends, debt hotspots |
| PR/push gate | bootstrap narrow (non-terminal) | `qodana-gate` | Yes (manual setup) | PR blocking + main/releases push alignment |
| Debt research | Observatory output | — | No | Campaigns, architecture review, baseline prep |

`cargo check` / `clippy` in [`.github/workflows/check.yml`](../../../.github/workflows/check.yml)
remain the compile authority. Qodana adds IDE semantics and maintainability
observations Qodana alone can surface.

## Hard guardrails

### A. Observatory must not be hollowed out

`observatory.yaml` is a **profile overlay only**. Suppressions live exclusively in
root [`qodana.yaml`](../../../qodana.yaml):

1. Shared path excludes (environment / false positive / archived lane)
2. Proven false-positive suppressions (`false_positive`)
3. Environment-limitation suppressions (`environment_limitation`)

Forbidden: extra `enabled: false`, `base: empty`, or `include` allowlists on
observatory “to make reports easier to read.” Non-blocking ≠ permission to
narrow inspections.

### B. Gate narrowing order

Gate profiles must evolve in this order (no skipping steps):

1. Start from `profile.base: qodana.recommended`
2. Apply shared path excludes from root `qodana.yaml` (in-config `NOTICE` there)
3. Mark debt-family inspections advisory-only in gate (do not `enabled: false`
   globally); prefer severity / failure policy over disabling
4. Temporary bootstrap narrowing: remove individual inspections from gate only,
   each annotated `# NOTICE(qodana-bootstrap-gate): non-terminal; removal condition: …`
5. Never freeze step 4 as the long-term model

Long-term gate is a **subtractive derivative of recommended**, not a permanent
`empty + static allowlist`.

### C. Suppress in-config NOTICE

Every suppress (`exclude`, `enabled: false`, baseline entry) must have an
adjacent `# NOTICE(qodana-<category>): <reason>` in YAML. This document is an
index; it does not replace in-config notices. Review blocks suppressions
without NOTICE.

## Suppress Contract

| Field | Requirement |
|-------|-------------|
| Category | `false_positive` / `environment_limitation` / `archived_lane_exemption` / `owner_accepted_debt` |
| Scope | path-level first → inspection+path → inspection-wide → profile-wide (last resort) |
| Evidence | `cargo check`, tests, domain boundary doc, or tool limitation with link/commit |
| Exit condition | Tool upgrade re-review, lane reactivation, baseline campaign done, or expiry date |

Forbidden: suppressing “because this report is large”; putting false positives
into baseline.

## Inspection tiers

### PR gate — blocking candidates

| Inspection | Rule |
|------------|------|
| `RsUnresolvedPath` | Must be visible in gate |
| `RsUnusedImport` | Block by default |
| `RsLiveness` / `RsUnreachablePatterns` / `RsConstantConditionIf` | Block true positives; false positives → path exclude + Contract |
| `RsDeprecation` | Archived lanes via path exclude (e.g. `crates/auv-game-minecraft/**`); never disable inspection globally for archived code |
| `CargoUnusedDependency` | Allowed in gate; run `cargo check -p <crate>` before removing deps |

### Observatory / debt — observe, do not block PR

- `DuplicatedCode`, `RsUnnecessaryQualifications`, and other maintainability families
- Consume via Qodana Cloud trends, SARIF, and focused campaigns
- Do not `enabled: false` these globally to quiet PRs

### Registered false positives (path exclude; inspections stay on)

| Inspection | Path | Category |
|------------|------|----------|
| `RsFunctionCannotHaveSelf` | swift-bridge `binding.rs` (overlay + driver-macos) | `false_positive` |
| `TomlUnresolvedReference` | inference optional-feature `Cargo.toml` files | `false_positive` |
| `RsConstantConditionIf` | `auv-driver-macos/src/session.rs` test stubs | `false_positive` |
| `RsUnreachablePatterns` | `auv-inference-ort/src/lib.rs` cfg catch-all | `false_positive` |
| `RsLiveness` | `src/inference_recognition.rs` `unreachable!` macro | `false_positive` |
| `RsDeprecation` | `crates/auv-game-minecraft/**` | `archived_lane_exemption` |

## Observatory minimum deliverables

Each `qodana-observatory` run must:

1. Upload to **Qodana Cloud** (`QODANA_TOKEN`) for trend/history
2. Upload **SARIF** to GitHub Code Scanning (`upload-sarif` on `qodana.sarif.json`)
3. Retain a **workflow artifact** (`upload-result` or explicit upload) for ≥ 14 days

Infra success → job green. Debt hotspots remain visible in Cloud / SARIF.
Do not use `continue-on-error` to skip uploads.

## Baseline policy (documented; implementation deferred)

Enable baseline only after gate is stable ≥ 2 weeks.

| Allowed in baseline | Forbidden in baseline |
|---------------------|----------------------|
| Confirmed existing debt with owner/campaign | Proven false positives |
| Gate-stable, gate-family issues | Archived-lane expected signals |
| | Environment/bootstrap problems |

When enabled: maintainer exports `qodana.sarif.json` from Cloud, commits to
repo, gate uses `--baseline qodana.sarif.json --fail-threshold N` to block only
**new** gate-family problems.

## CI trigger matrix

| Event | `qodana-observatory` | `qodana-gate` |
|-------|----------------------|---------------|
| `pull_request` | skipped | runs |
| `push` → `main` / `releases/*` | runs | runs |
| `schedule` (weekly) | runs | **skipped** |
| `workflow_dispatch` | runs | **skipped** |

Branch protection (repo setting, not in this repo): bind required check to
`qodana-gate` after merge. Observatory does not block merges.

## Config layout (shared source + profile overlay)

Qodana reads repository-root [`qodana.yaml`](../../../qodana.yaml) by default.
CI jobs pass `--profile-path` to select the inspection profile overlay only;
`--config` is **not** used (it would replace the entire config).

```
qodana.yaml                           # single source: bootstrap + exclude + NOTICE
.qodana/profiles/observatory.yaml     # profile overlay: qodana.recommended
.qodana/profiles/gate.yaml            # profile overlay: bootstrap narrow set + NOTICE
.github/workflows/qodana_code_quality.yml
  observatory: --profile-path .qodana/profiles/observatory.yaml
  gate:        --profile-path .qodana/profiles/gate.yaml --fail-threshold 0
```

Profile resolution order (JetBrains): CLI `--profile-path` overrides
`qodana.yaml` `profile.*`; bootstrap and exclude always come from root
`qodana.yaml` when `--config` is omitted.

## Open PR convergence

| PR | Keep | Drop |
|----|------|------|
| #69 | Path excludes with evidence + NOTICE | `qodana.sanity` / empty-only gate profile |
| #71 | Real hygiene (unused import/dep, cfg, fmt) | Profile narrowing philosophy |

Cherry-pick hygiene only when each hunk is justifiable without Qodana (not a
tool concession).

## References

- [Qodana YAML](https://www.jetbrains.com/help/qodana/qodana-yaml.html)
- [Rust linter](https://www.jetbrains.com/help/qodana/rust.html)
- [GitHub Actions](https://www.jetbrains.com/help/qodana/github.html)
- [Baseline](https://www.jetbrains.com/help/qodana/baseline.html)
