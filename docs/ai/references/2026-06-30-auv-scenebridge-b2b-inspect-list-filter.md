# SceneBridge B2b: Inspect viewer client-side list filter

Date: 2026-06-30
Status: **landed** (viewer-only run list filters)

## Summary

B2b adds **client-side** run list filtering in the inspect viewer sidebar. Users
toggle chips (`failed` / `stale` / `limits`) to narrow the run list without new
`/runs` API parameters. Filters consume existing per-row `view_parser_summary`
from B1c plus run metadata (`status_code`).

## Priority gate

B2b is **second priority** — promote only when the real pain is “too many runs;
finding failed / stale / limits is slow.” In-run navigation is B2a.

## Owner decisions

| Decision | Choice |
|----------|--------|
| **failed** | `latest_verification_status === "failed"` **OR** `status_code === "error"` |
| **Filter mode** | Multi-select **AND** across active chips |
| **Empty selection** | Show all runs (all chip active) |
| **Pure helpers** | `visibleRunsForList(runs, filters)` — no global state reads |
| **loadRuns failure** | Hide banner, `#run-count` → `—`, disable chips (retain `runListFilters` in memory) |
| **Active run hidden** | `#run-list-filter-banner` sibling of `#view-parser-proof` / `#main-body` |

## Filter predicates

| Chip | When active, run matches if |
|------|----------------------------|
| `failed` | `summary.latest_verification_status === "failed"` OR `run.status_code === "error"` |
| `stale` | `summary.latest_outcome === "stale"` |
| `limits` | `summary.has_known_limits === true` |

`summary` = `run.view_parser_summary || {}`. Empty `filters` Set → all runs visible.

## Known limitations

| Limitation | Behavior |
|------------|----------|
| **not_found vs failed** | Row badge may show `not_found`; **failed chip does not match** not_found-only rows unless verification failed or run errored. |
| **Latest-only stale / verification** | `latest_outcome` / `latest_verification_status` come from the **last** resolution only. Earlier stale on multi-resolution runs may be missed. |
| **Degraded list rows** | Default summary on read/build failure: `failed` may still hit via `status_code === "error"`; `stale` / `limits` never match. |
| **Running runs** | In-flight runs usually have empty summary; no chip matches until finish + list refresh. |
| **Verification optional fields** | `latest_verification_status` null + `status_code === "ok"` → failed chip does not match (e.g. not_found + passed verification). |

## Non-goals

- No cross-run compare
- No server-side list filter / query params
- No list sort
- No `ViewParserListSummary` schema changes
- No new inspect endpoints
- No URL persistence for filter state
- No A-line producer changes

## Key files

- [`src/inspect_server_viewer.html`](../../../src/inspect_server_viewer.html) — filters, banner, self-test
- [`src/inspect_server/mod.rs`](../../../src/inspect_server/mod.rs) — `viewer_renders_view_parser_list_filter_hooks`
- [B1 handoff](2026-06-30-auv-scenebridge-b1-inspect-viewer-consumption.md) (list badges + `view_parser_summary`)
- [B2a handoff](2026-06-30-auv-scenebridge-b2a-inspect-diagnostic-links.md) (in-run diagnostic links)

## Validation

```sh
cargo fmt --check
cargo check
cargo test viewer_renders_view_parser_list
git diff --check
```
