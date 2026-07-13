# AUV Reference Index

`docs/ai/references/` is organized by **responsibility folder**. Each folder has its own `INDEX.md`.

Flat filenames at this root are **tombstones** that point at the new path (or merged durable note). Prefer folder indexes for navigation.

## Naming

New notes:

```text
<responsibility-folder>/YYYY-MM-DD-<descriptive-slug>-<doc-type>.md
```

Do not put engineering slice codes (`a2`, `p14`, scan-step codes, etc.) in navigation labels or new filenames.

## Document types

| Suffix / type | Purpose |
|---|---|
| `design` | Accepted or pending feature / boundary design |
| `spec` | Approved or pending scope statement |
| `plan` / `implementation-plan` | Implementation steps and dependency order |
| `handoff` | Slice completion handoff |
| `evidence` / `evidence-pack` | Reproducible evidence, benchmarks, smoke records |
| `closure` / `live-closure` / `reference` | Gate closeout or folded durable reference |
| `review` / `graduation-review` / `verdict` | Review, graduation gate, verdict |
| `roadmap` | Multi-slice roadmap (does not auto-approve downstream work) |
| `matrix` / `taxonomy` / `inventory` | Tables, taxonomies, retirement inventories |
| `freeze` / `acceptance` | Phase freeze or acceptance |
| `note` | Durable note without a stronger type |

## Responsibility folders

| Folder | Status | Responsibility | Notes |
|---|---|---|---|
| [`runtime/`](runtime/INDEX.md) | Active | Execution, contract, action seam, admission, query readiness | |
| [`invoke-cli/`](invoke-cli/INDEX.md) | Active | Invoke routing, CLI handlers, catalog | |
| [`session-api/`](session-api/INDEX.md) | Active | Session API, proto, MCP frontend | |
| [`inspect/`](inspect/INDEX.md) | Active | Run recording, inspect viewer, trace | |
| [`driver/`](driver/INDEX.md) | Active | Platform drivers, input, window, permissions | |
| [`view-memory/`](view-memory/INDEX.md) | Active | View-parser IR and view memory | |
| [`scan/`](scan/INDEX.md) | Active | Temporal scan / surface observation | |
| [`scenebridge/`](scenebridge/INDEX.md) | Active | Cross-app scene identity / grounding | |
| [`recognition/`](recognition/INDEX.md) | Active | RecognitionResult, detectors | |
| [`apps/netease-music/`](apps/netease-music/INDEX.md) | Product | Netease Cloud Music app-local commands | |
| [`apps/qqmusic/`](apps/qqmusic/INDEX.md) | Historical | QQ Music probes | |
| [`apps/minecraft/`](apps/minecraft/INDEX.md) | Paused | Minecraft spatial probe | |
| [`apps/osu/`](apps/osu/INDEX.md) | Probe | osu consumption / benchmark | |
| [`apps/balatro/`](apps/balatro/INDEX.md) | Probe | Balatro consumption probe | |
| [`apps/godot/`](apps/godot/INDEX.md) | Proposed | Godot dev-time observation | |
| [`apps/game-observe/`](apps/game-observe/INDEX.md) | Observe-only | Steam / STS fixtures | |
| [`archive/skill-bundle/`](archive/skill-bundle/INDEX.md) | Retired | SkillBundle / recipe retirement | |
| [`archive/phase-history/`](archive/phase-history/INDEX.md) | Historical | Early phase freeze / acceptance | |
| [`archive/ax-copilot/`](archive/ax-copilot/INDEX.md) | Archived | macOS AX copilot vertical | |
| [`ops/`](ops/INDEX.md) | Mixed | Setup, tooling, cross-cutting notes | |

## Quick entry

| You want… | Start here |
|---|---|
| Shared vocabulary | [`../../TERMS_AND_CONCEPTS.md`](../../TERMS_AND_CONCEPTS.md) |
| Core roadmap | [`runtime/2026-06-13-core-roadmap.md`](runtime/2026-06-13-core-roadmap.md) |
| Invoke / CLI design | [`invoke-cli/2026-06-11-cli-invoke-driver-console-design.md`](invoke-cli/2026-06-11-cli-invoke-driver-console-design.md) |
| Inspect viewer design | [`inspect/2026-05-19-trace-run-inspect-design.md`](inspect/2026-05-19-trace-run-inspect-design.md) |
| Temporal scan | [`scan/INDEX.md`](scan/INDEX.md) |
| Session API / MCP | [`session-api/INDEX.md`](session-api/INDEX.md) |
| Design system / viewer UI | [`../../design/README.md`](../../design/README.md) |
| Agent writing rules | [`../../../AGENTS.md`](../../../AGENTS.md) |
| Archived AX copilot | [`../../archive/verticals/ax-copilot/`](../../archive/verticals/ax-copilot/) |

## Evidence attachments

- [`evidence/2026-05-14-qqmusic-search-ocr-anchor/`](evidence/2026-05-14-qqmusic-search-ocr-anchor/)
- [`evidence/2026-05-15-qqmusic-play-visible-anchor/`](evidence/2026-05-15-qqmusic-play-visible-anchor/)
- [`evidence/2026-06-11-mcp-read-chain/`](evidence/2026-06-11-mcp-read-chain/)
- [`evidence/2026-06-30-scenebridge-netease-sidebar/`](evidence/2026-06-30-scenebridge-netease-sidebar/)

## Related

| Path | Purpose |
|---|---|
| [`docs/README.md`](../../README.md) | Documentation layout overview |
| [`docs/TERMS_AND_CONCEPTS.md`](../../TERMS_AND_CONCEPTS.md) | Shared vocabulary |
| [`docs/ai/explanations/`](../explanations/) | Tutorials and interactive explainers |
| [`docs/design/`](../../design/) | Vendored design system |
| [`docs/archive/verticals/`](../../archive/verticals/) | Archived vertical proofs |

## Maintenance

1. Place new reference under the owning responsibility folder.
2. Add one line to that folder’s `INDEX.md`.
3. Add or adjust a row in this root index only when a **new folder** appears.
4. When folding intermediate handoffs, leave a root tombstone that points at the merged durable note.
