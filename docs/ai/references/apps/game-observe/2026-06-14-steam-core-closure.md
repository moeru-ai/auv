# C3 Steam Library Through Core Invoke — Closure Record

Date: 2026-06-14
Status: **C3a/C3b/C3c completed locally and validated; ready for commit/push selection**
Roadmap anchor: `docs/ai/references/runtime/2026-06-13-core-roadmap.md`
Short-term plan: `docs/ai/references/runtime/2026-06-14-core-short-term-plan.md`
Frontend convention: `docs/ai/references/ops/2026-06-11-frontend-convention-v0.md`

## Scope closed in this document

This closure record covers the whole C3 lane:
- `C3a` honest backend rehome for `steam.library.list.v0`
- `C3b` thin-frontend boundary so `auv-steam` bin and the core command share the same library entry
- `C3c` regression coverage that pins structured evidence and inspect shape

It does **not** include C2e or any broader Runtime deletion work.

## What changed

### C3a — honest backend

`steam.library.list.v0` no longer routes through `fixture.observe`.

Changed files:
- `src/driver/steam.rs`
- `src/driver/mod.rs`
- `src/driver/fixture.rs`
- `src/catalog.rs`
- `src/driver/macos/tests.rs`

Landed behavior:
- New `steam.local` driver owns `steam_library_list` in `src/driver/steam.rs`.
- `default_driver_registry()` registers `SteamLocalDriver` alongside `FixtureObserveDriver` and `LegacyMacosCommandDriver` in `src/driver/mod.rs`.
- `fixture.observe` now rejects `steam_library_list` and is back to being a pure fixture driver in `src/driver/fixture.rs`.
- `steam.library.list.v0` now resolves to `driver_id = "steam.local"` in `src/catalog.rs`.

Behavior intentionally preserved:
- The command still reuses `auv-steam` library code.
- The summary, backend string, signals, and artifact role stay aligned with prior evidence shape.

### C3b — thin frontend / shared library entry

The `auv-steam` binary and the core command now share the same library entry.

Changed files:
- `crates/auv-steam/src/app.rs`
- `crates/auv-steam/src/lib.rs`
- `crates/auv-steam/src/cli.rs`
- `src/driver/steam.rs`

Landed behavior:
- Added `query_local_library_apps(query)` in `crates/auv-steam/src/app.rs`.
- Re-exported it from `crates/auv-steam/src/lib.rs`.
- `crates/auv-steam/src/cli.rs` now calls `query_local_library_apps(query)` instead of directly doing its own `Steam::locate() + library_apps()` sequence.
- `src/driver/steam.rs` also calls the same `query_local_library_apps(query)` entry.

Result:
- `auv-steam` bin is now a presentation shell over the same library entry the core command uses.
- No parallel executor or duplicate Steam query path remains inside the bin/core split.

### C3c — structured evidence + inspect shape pinned

Changed file:
- `src/mcp.rs`

Landed behavior:
- The existing MCP integration test for `steam.library.list.v0` now asserts structured evidence from the `invoke` JSON:
  - one artifact exists
  - artifact `role == "steam-library-list"`
  - artifact `path` contains `steam-library-list.json`
- The same test now asserts inspect text shape for the run:
  - summary line is present
  - resolved command path shows `steam.local.steam_library_list`
  - backend line shows `steam.local_appmanifest.library-list`
  - artifact capture text includes `kind=steam-library-list`
  - inspect text includes the artifact path and the driver notes (`resolvedSource=local_appmanifest`, `appCount=`)
  - empty sections remain explicit (`Verifications: - none`, `Observations: - none`)

Important review-driven refinement:
- The first draft pinned `artifact_0001`, which was judged too brittle.
- The final test pins stable shape/semantics instead of recording-order-specific ids.

## Validation run in this session

Passed:
- `cargo fmt --check`
- `cargo check`
- `cargo test`
- `cargo check -p auv-steam -p auv-cli`
- `git diff --check`

Focused regression checks passed:
- `cargo test fixture_driver_rejects_steam_library_operation -- --nocapture`
- `cargo test default_catalog_routes_steam_library_through_steam_local -- --nocapture`
- `cargo test driver_registry_stores_and_retrieves_drivers -- --nocapture`
- `cargo test -p auv-steam library_ls_uses_shared_local_query_entry -- --nocapture`

Real smoke checks passed:
- `cargo run -p auv-steam -- library ls --format summary`
- `cargo run -- invoke steam.library.list.v0`
- `cargo run -- inspect run_1781426180084_9816_0`
- `cargo run -- inspect run_1781426851953_33701_0`

Example successful run ids recorded during C3 closure:
- `run_1781426180084_9816_0`
- `run_1781426851953_33701_0`
- `run_1781427107552_44124_0`

Example confirmed artifact path:
- `.auv/runs/run_1781427107552_44124_0/artifacts/artifact_0001_steam-library-list.json`

## Review summary

Subagent review was run in small waves with main-thread arbitration.

Accepted conclusions:
- `C3a`: no material issues after adding explicit fixture rejection + registry coverage.
- `C3b`: no material issues; bin and core command now share the same library entry.
- `C3c`: one medium-confidence review note was accepted — avoid pinning `artifact_0001`; the test was updated to assert stable evidence shape instead.

Final state after fix/re-review:
- no accepted blockers remain for C3.

## Follow-up observations (not part of C3)

- `steam.local` is intentionally Steam-specific, not a generic `local.read` driver.
  Generalization stays deferred until a second approved API/file-read consumer exists.
- The core invoke path still uses `LibraryQuery::default()` inside `src/driver/steam.rs`.
  Input-driven query projection is intentionally deferred until an owner-approved
  slice defines how `DriverCall.inputs` should map onto `LibraryQuery`.

## One-line truth of repo state now

C3 is locally closed: `steam.library.list.v0` has an honest backend, thin frontend reuse through one shared library entry, and regression coverage that fixes structured evidence plus inspect shape.
