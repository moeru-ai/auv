# Inspect composition S4–S5 migration — auv-cli library core-only + server/viewer parity

Date: 2026-07-13
Status: **S3a + S3b + S4 + S5 complete and root-reviewed** (`COMPLETE_READY_FOR_PR`)

## Final architecture (locked)

| Package | Owns |
|---|---|
| `auv-inspect-model` | Neutral `InspectSection` / `InspectDocument` / `InspectComposer`, `ArtifactRefLineage`, MIME policy, generic store/JSON helpers |
| `auv-cli` | Library-only core: contract/runtime/session/core inspect & MCP; **CorePrefixSection + CoreSuffixSection** only for full-run text; **no** `auv-game-*` / `auv-godot` package deps; **zero** `auv_game_*` in `src/`; core `RootInspectReadProjection` consumes injected core composer for document/text |
| `auv-product` | Bins `auv` / `auv-minecraft` / `auv-osu` / `auv-godot`; CLI; verticals; product composer (locked order); query-wired bridges (S3b); product projection (composer + named JSON extensions) |
| `auv-game-*` | Domain types + ordinary run_read/inspect section factories; must not depend on `auv-cli` |
| `auv-inspect-server` | Generic HTTP routes: enrichment, `/inspect`, `/inspect/document`, `/extensions/{extension}`; no Minecraft-first routes |

Workspace: `default-members = [".", "crates/auv-product"]`, product `default-run = "auv"` so
bare `cargo test` / `cargo run` cover both the core library (`auv-cli`) and product
bins; `cargo run --quiet -- invoke --help` keeps working from repo root.

## LOCKED golden render order

1. `core_prefix` — Run header through Detector Recognition
2. minecraft primary — `auv_game_minecraft::inspect_sections_primary`
3. balatro — `auv_game_balatro::inspect_sections`
4. minecraft quality+spatial — `auv_game_minecraft::inspect_sections_quality_spatial`
5. osu A — Semantic / Spatial Query / Action Readiness
6. osu query-wired (**PRODUCT**) — OperationResult adapter
7. osu B — Detection Eval Witness / Quality
8. minecraft query-wired (**PRODUCT**) — OperationResult adapter
9. `core_suffix` — View parser proof + Scene state

## Behavior changes (S3b + S4 + S5)

### Deleted / moved out of `auv-cli` library

- Root bins / CLI frontend / verticals tree removed from `src/` (now
  `crates/auv-product`).
- Root product/default donor composer removed. Core
  `build_core_inspect_composer` emits **only** `CorePrefixSection` +
  `CoreSuffixSection`.
- Donor ordinary list/extract/render bodies live in `auv-game-*` (S3a).
- Stale product inspect/run_read shells (`legacy`, per-game inspect modules under
  product that duplicated game crates) are gone; product inspect keeps only
  composer assembly + query-wired bridges + goldens.
- Product `run_read` exposes query-wired summaries only; ordinary game readers
  and core read helpers are crate-local wiring, not a public compatibility facade.
- `LegacyFullRunSection` / `legacy_body` removed after S3a split (historical docs
  only).

### Where product composer lives

- `auv_product::product_inspect::build_product_inspect_composer` →
  `auv_product::inspect::sections::build_product_inspect_composer`
- Product CLI + product MCP + product inspect-server projection share that
  composer via explicit injection (`serve_stdio_with_composer` /
  `ProductInspectReadProjection::with_composer`). No MCP/server stack fork.
- Core `McpServer::new` / `serve_stdio` stay core-only.

### S3b — query-wired partial graduation (stays in product)

- Adapters remain in `crates/auv-product/src/run_read/query_wired_live_action.rs`
  and product inspect sections `query_wired_{minecraft,osu}.rs`.
- **Not** moved whole-file into game crates.
- Neutral eligibility-to-`readiness_class` mapping lives in
  `auv-query-readiness`; product-local source-ref projection remains in
  `run_read/query_wired_projection.rs` (no `OperationResult` types).
- Donor event names / summary structs / OperationResult verification projection
  stay local in the adapter file.
- Code-site NOTICE/TODO: blocked on OperationResult (+ verification/failure)
  ownership approval before full donor graduation.

### Projection / packaging

- Core `RootInspectReadProjection` has no Minecraft JSON extension; product wraps
  via `ProductInspectReadProjection`.
- Root `auv minecraft|osu|godot` tombstoned; live donor bins are `auv-minecraft` /
  `auv-osu` / `auv-godot` (subprocess lock: `crates/auv-product/tests/donor_cli_migration.rs`).
- Root package `[dependencies]` lists no `auv-game-*` / `auv-godot` (workspace
  `members` may still include those crates for the workspace graph).

### S5 — Server/viewer generalization (landed)

- `InspectReadProjection` consumes composer-backed `inspect_document` /
  `inspect_text` (wire types `InspectDocumentWire` / `InspectSectionWire`); a
  projection without an inspect composer returns route-level 404 rather than a
  misleading server error.
- Shared server routes:
  - `GET /runs/{id}/inspect` — composed text
  - `GET /runs/{id}/inspect/document` — ordered sections
  - `GET /runs/{id}/extensions/{extension}` — named JSON extensions
- Removed first-class `GET /runs/{id}/minecraft-quality-baseline-report`.
- Viewer fetches quality baseline via the generic extension key
  `minecraft-quality-baseline-report` (donor data stays product-registered).
- Frontend parity that is true now: **product CLI text inspect**, **product MCP
  `run_inspect`**, and **product inspect-server projection** use the same product
  composer factory and locked section set; each frontend injects its composer
  `Arc` through all text/document paths for its lifecycle. Viewer UI still
  renders donor-specific artifact cards by artifact role (legitimate vertical
  UI); only the shared HTTP route hardcoding was removed.

## Intentionally deferred (code-site NOTICE required)

- **S3b full graduation**: whole query-wired adapter file move into game crates —
  blocked on OperationResult ownership.
- Viewer may still contain Minecraft-named artifact-role helpers for donor cards;
  that is UI role matching, not a shared first-class route. Further viewer
  role-table generalization is a separate slice if desired.

## Verification strategy

**Do use:**

- `rg 'auv_game_' src/` → empty (root library modules)
- `tests/core_package_manifest_no_game_deps.rs` (manifest assertion + src scan)
- `cargo test -p auv-cli`
- `cargo test -p auv-product` (goldens unchanged unless `AUV_UPDATE_INSPECT_GOLDENS=1`)
- `cargo test -p auv-query-readiness`
- `cargo test -p auv-inspect-server` (including generic extension success,
  missing-run 404, and unknown-extension 404)
- `cargo run --quiet -- invoke --help` (product default bin)
- `rg 'minecraft-quality-baseline-report' crates/auv-inspect-server/src/server.rs` → only test asserting 404 / gone
- `rg LegacyFullRunSection` → historical docs only

**Do not use as proof:**

- Fake / misleading `cargo tree -p auv-cli --lib` tricks

Workspace baseline `auv-driver-macos` Native* failures are out of slice.

## Collabi

Writer check-in to Collabi timed out / unpaid-invoice interrupted agents;
recorded and continued without pretending success.
