# AUV Documentation

AUV docs are split by **purpose**. Reference notes are further split by **responsibility folder** plus per-folder indexes.
Canonical project contract remains [`AGENTS.md`](../AGENTS.md).

## Layout

```text
docs/
├── README.md                 ← you are here
├── TERMS_AND_CONCEPTS.md     ← shared vocabulary (update when contracts change)
├── ai/
│   ├── references/           ← durable design / handoff / evidence notes
│   │   ├── INDEX.md          ← responsibility-folder index
│   │   ├── <responsibility>/ ← e.g. runtime/, scan/, apps/netease-music/
│   │   │   └── INDEX.md
│   │   ├── evidence/         ← evidence-pack attachments (json/png/txt)
│   │   └── YYYY-MM-DD-*.md   ← tombstones pointing at new paths
│   └── explanations/         ← tutorials, interactive HTML
├── design/                   ← vendored design system + viewer/cli mock
├── archive/verticals/        ← archived vertical proofs (not active roadmap)
└── notes/<owner>/            ← personal drafts (do not commit by default)
```

## Where to put what

| Content | Location |
|---|---|
| In-progress design / handoff / evidence | `docs/ai/references/<responsibility>/YYYY-MM-DD-<slug>-<type>.md` |
| Teaching / walkthrough / interactive demo | `docs/ai/explanations/` |
| New shared term | `docs/TERMS_AND_CONCEPTS.md` |
| Finished vertical that must not bias roadmap | `docs/archive/verticals/<name>/` + old-path tombstone |
| Personal exploration / local logs | `docs/notes/<owner>/` (commit only if owner asks) |
| UI tokens / viewer mock | `docs/design/` |

After adding a reference: add one line to that folder’s [`INDEX.md`](ai/references/INDEX.md) (and the folder’s own `INDEX.md`).

## Active vs archive

| Category | Meaning | Examples |
|---|---|---|
| **Core responsibilities** | invoke, runtime, inspect, driver, view-memory, scan, session-api | `runtime/`, `inspect/`, `scan/` |
| **Apps / probes** | app-local crates or consumption probes | `apps/netease-music/`, `apps/minecraft/` |
| **Archive** | retired surfaces, frozen phases, archived verticals | `archive/*`, `docs/archive/` |

`AGENTS.md` requires: `candidate-action` / macOS AX copilot must **not** be extended as an active product lane.

## Quick entry

| You want… | Start here |
|---|---|
| Shared vocabulary | [`TERMS_AND_CONCEPTS.md`](TERMS_AND_CONCEPTS.md) |
| Browse references by responsibility | [`ai/references/INDEX.md`](ai/references/INDEX.md) |
| Core roadmap | [`ai/references/runtime/`](ai/references/runtime/) |
| Invoke / CLI design | [`ai/references/invoke-cli/`](ai/references/invoke-cli/) |
| Inspect viewer design | [`ai/references/inspect/`](ai/references/inspect/) |
| Design system / viewer UI | [`design/README.md`](design/README.md) |
| Agent writing rules | [`../AGENTS.md`](../AGENTS.md) |
| Archived AX copilot | [`archive/verticals/ax-copilot/`](archive/verticals/ax-copilot/) |

## Reference volume

- Responsibility folders under [`ai/references/`](ai/references/) with per-folder indexes
- Root-level dated `*.md` files are tombstones after the folder reorg
- Full navigation: [`ai/references/INDEX.md`](ai/references/INDEX.md)
