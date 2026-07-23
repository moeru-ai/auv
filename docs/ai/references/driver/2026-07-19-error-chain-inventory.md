# AUV Error Chain Inventory

Date: 2026-07-19
Responsibility: driver (error semantics), runtime (operation failure mapping)
Type: inventory / baseline
Milestone: Workstream 2 / PR 4

## Purpose

Document the current error-flattening points and String-error boundaries across
core-maintained crates before PR 5 (the typed-error vertical slice) begins
reclassification. This is the baseline that lets PR 5 prove it changed error
*classification* without changing error *messages* (behavior-preserving).

Scope: core crates only — `auv-driver-macos`, `auv-driver-common`,
`auv-tracing-driver`, `auv-inspect-server`, `auv-cli-invoke`, root `src/`.

## Summary of findings

- **4 public `Result<T, String>` type aliases** cross core crate boundaries
  (milestone-banned pattern, still present).
- **53 `native/` functions** return `AuvResult<_>` (String errors); only **3**
  (in `accessibility.rs` + the #114-upgraded `inspect_ax_node_path`) return
  `DriverResult`.
- **1 critical flattening function** (`native_error_to_auv`) converts structured
  `NativeDriverError { operation, message, recovery_hint }` into a formatted
  String at the Swift→Rust decode boundary.
- **1 read-side silent error discard** (`inspect-server/server.rs:427`
  `store.read_run().ok()`) collapsed genuine read failures to `None` at
  inventory time; fixed by [#124](https://github.com/moeru-ai/auv/pull/124).
- **0 production String-matching branches** for error classification — all
  `.contains()` calls are in test assertions, not control flow.
- **#114 as a half-typed precedent**: `inspect_ax_node_path` signature is
  `DriverResult`, but its decode layer still calls `native_result` (String
  template) + wraps the String in `DriverError::Backend { message }` at the
  seam. The *message* is typed; the *decode* is not. PR 5's job is to push
  typing into decode.

## 1. Public `Result<T, String>` boundaries (milestone-banned)

Milestone Workstream 2 explicitly bans `Result<T, String>` at public crate
boundaries. Four type aliases still exist:

| Crate | Alias | File | Public surfaces using it |
|---|---|---|---|
| `auv-tracing-driver` | `AuvResult<T>` | `error.rs:1` | `LocalStore` methods (`new`, `read_run`, `list_runs`, `artifact_file`); `RunBuilder` (`start_span`, `finish_span`) |
| `auv-inspect-server` | `InspectResult<T>` | `lib.rs:18` | All public functions in the crate root + `server.rs` handlers |
| `auv-driver-macos` | `AuvResult<T>` | `types.rs:5` | All `native/` module functions (53 fns); `#[doc(hidden)]` re-exports via `pub mod native` |
| `auv-cli-invoke` | `InvokeCommandResult` | `command.rs:73` | The `InvokeCommand::execute` trait method (CLI handler contract) |

The `auv-driver-macos` native surface is marked `#[doc(hidden)]` + gated by
`TODO(driver-crates)` as temporary-public during migration, so it's
*intentional* but still violates the boundary rule. The other three are
unqualified public APIs.

## 2. The flattening chokepoint: `native_error_to_auv`

Swift native functions (AX / capture / clipboard / input / OCR / pointer /
window) return responses with optional `error_message` and `recovery_hint`
fields. On the Rust decode side, these flow through:

```rust
// crates/auv-driver-macos/src/native/error.rs
pub struct NativeDriverError {
  pub operation: String,    // e.g. "inspect_ax_node"
  pub message: String,      // Swift's error_message
  pub recovery_hint: String, // Swift's recovery_hint
}

pub fn native_error_to_auv(error: NativeDriverError) -> String {
  format!("macos native {} failed: {}; recovery={}", error.operation, error.message, error.recovery_hint)
}

pub fn native_result<T>(...) -> AuvResult<T> {
  match value {
    Some(v) => Ok(v),
    None => Err(native_error_to_auv(NativeDriverError { ... })),
  }
}
```

Every `decode_*_response` function in `native/ax_tree.rs`, `native/ocr.rs`,
etc. calls `native_result(operation, value, error_message, recovery_hint)` when
the Swift response signals failure. The structured `NativeDriverError` is
immediately flattened to a String template.

**Consequence**: callers receive `"macos native inspect_ax_node failed: AX
inspection path must begin with 0; got 1.2; recovery=capture a fresh AX tree
and retry the inspection"` — a formatted String with no machine-readable
variant, no separate `operation` / `message` / `recovery` fields, and no way to
distinguish "stale path" from "permission denied" without parsing the message
text.

## 3. Current `DriverError` coverage

`auv-driver-common/src/error.rs` defines a structured enum:

```rust
pub enum DriverError {
  Unsupported { operation: &'static str },
  NotFound { target: String },
  PermissionDenied { permission: &'static str, recovery: Option<String> },
  InvalidInput { message: String },
  Backend { message: String },
}
```

But only **3 of 53 native functions** return `DriverResult`:
- `accessibility.rs`: `capture_app_tree`, `focus_node_path`, `focus_text_by_query`, `verify_text` — fully typed, use `DriverError::NotFound` / `InvalidInput` / `Backend` appropriately.
- `ax_tree.rs`: `inspect_ax_node_path` (#114) — returns `DriverResult`, but its decode still produces a String, wrapped at the seam with `.map_err(|message| DriverError::Backend { message })`.

The other 50 native functions return `AuvResult<_>` (String). A few higher-level
`session.rs` methods (`scroll`, `click_at`, `scroll_global_hid`) are typed
`DriverResult` and convert the String errors from native calls with
`.map_err(backend)` where `backend` is a closure wrapping
`DriverError::Backend { message }`.

**Pattern**: the outer signature gets typed; the decode layer stays String;
conversion happens at the call site. This pushes typing "up" but doesn't push
it "down" into decode — so error *classification* (stale path vs role mismatch
vs out-of-range) remains impossible.

## 4. String-matching for error control flow

**Good news**: no production code branches on `error.contains("stale")` or
similar. All `.contains()` calls found are in **test assertions** (`#[test]`
functions checking that parse errors include expected substrings).

One caveat: `auv-cli-invoke/src/commands/media_control.rs:122` has an assertion
inside a command handler, but it's defensive (`assert!(error.contains("typed
media control API"), ...)`) — it crashes on unexpected error text, it doesn't
silently alter behavior.

## 5. Silent error discards on evidence paths

Milestone Workstream 2 explicitly forbids `.ok()` / `unwrap_or_default()` on
evidence/artifact read paths. Found **1 violation at inventory time**, fixed
by [#124](https://github.com/moeru-ai/auv/pull/124) (2026-07-21):

- `crates/auv-inspect-server/src/server.rs:427` used to read:
  ```rust
  let mut snapshot = state.store.read_run(&run_id).ok();
  ```
  A `LocalStore::read_run` failure (file I/O error, parse error, permission
  denied) was collapsed to `None`, which the HTTP handler treated as "run not
  found" — masking the real failure reason. Concretely, an incremental
  update against an existing-but-corrupted run failed `apply_update`'s
  `missingRunStarted` check and was reported as a `409 runConflict`
  (a plausible-looking but wrong client-protocol diagnosis), and a
  `runStarted` update would have silently reconstructed a fresh empty
  snapshot over the corrupted run directory.

  #124 checks for `run.json` explicitly before deciding a run hasn't
  started — the only case a missing snapshot is legitimate — and propagates
  `read_run` failures as real errors (`500`, via `InspectHttpError::from_store`)
  once `run.json` exists. **0 violations remain** for this pattern as of
  #124.

## 6. The #114 precedent — half-typed, needs completion

PR #114 (`refactor(auv-driver-macos): return DriverResult from
inspect_ax_node_path`) changed the public signature:

```diff
-pub fn inspect_ax_node_path(...) -> AuvResult<AxNodeInspection>
+pub fn inspect_ax_node_path(...) -> DriverResult<AxNodeInspection>
```

But the decode layer (`decode_ax_node_inspection_response`) still returns
`AuvResult<_>` and calls `native_result`, producing the formatted String. The
function body does:

```rust
decode_ax_node_inspection_response(...)
  .map_err(|message| DriverError::Backend { message })
```

So the *interface* is typed, but the *decode* isn't. The `DriverError::Backend`
variant just wraps the String — no reclassification, no structured recovery
hint, no machine-readable cause. Callers still can't distinguish "path must
begin with 0" from "out of range" without parsing `message`.

**This is the gap PR 5 must close**: push typing into decode so the decode
layer itself constructs a classified `DriverError` variant (e.g.
`DriverError::InvalidInput` for parse failures, `DriverError::NotFound` for
stale paths), preserving the Swift error's structure (`operation`, `message`,
`recovery_hint`) without flattening to a template.

## 7. Error flow map (current state)

```text
Swift native function
  ↓ (returns NativeAxNodeInspectionResponse with error_message, recovery_hint)
decode_ax_node_inspection_response
  ↓ calls native_result(operation, None, error_message, recovery_hint)
native_error_to_auv
  ↓ formats NativeDriverError { operation, message, recovery_hint } into String
"macos native {operation} failed: {message}; recovery={recovery_hint}"
  ↓ (if the outer fn is typed)
.map_err(|message| DriverError::Backend { message })
  ↓
DriverError::Backend { message: "macos native inspect_ax_node failed: ..." }
  ↓ Display impl writes message as-is
CLI / inspect output: unparseable String
```

**What's missing at each layer**:
- Swift → Rust: the response fields are preserved, but decode immediately flattens them.
- Decode → public API: no classification (stale vs role-mismatch vs parse-error) — everything is `Backend { message }`.
- Public API → operation: `DriverError` exists but most functions don't use it.
- Operation → CLI: the CLI renders `DriverError::Display`, which is good, but can't distinguish causes programmatically.

## Recommendation for PR 5: the narrowest typed-error vertical slice

Pick **one AX native function** (e.g. `perform_ax_path_action` or
`set_ax_focused_path` — both share the `axResolveObservedPath` error contract
characterized in PR 2) and type it end-to-end:

1. Keep the Swift response fields (`error_message`, `recovery_hint`) as-is.
2. In the decode function (`decode_ax_action_response` or `decode_ax_focus_response`),
   **classify the error** by inspecting `error_message`:
   - "path must begin with 0" → `DriverError::InvalidInput`
   - "is out of range" or "tree likely shifted" → `DriverError::NotFound` (or a new `StaleObservation` variant)
   - "expected role X, got Y" → `DriverError::NotFound`
   - "permission denied" / "Accessibility" → `DriverError::PermissionDenied`
   - fallback → `DriverError::Backend`
3. Preserve `recovery_hint` in the appropriate `DriverError` field (`recovery`
   in `PermissionDenied`, as a message suffix in `InvalidInput`, etc.).
4. Update the public function signature to `DriverResult`.
5. Add a **characterization test** (like PR 2's harness) that locks the current
   error *messages* before changing classification, then a **classification test**
   that asserts the decode maps specific Swift messages to specific
   `DriverError` variants.

This proves the pattern works for one function without touching all 53. Once
approved, PR 6+ can fan out to the rest.

## Non-goals (deferred to later PRs)

- No global `Result<T, String>` → `DriverResult` migration (too large).
- No new `DriverError` variants yet (e.g. `StaleObservation` / `RoleMismatch`)
  — use existing variants first, propose new ones only if the current set can't
  express a real distinction.
- No operation-layer failure mapping (how `DriverError` maps to
  `OperationResult` / `VerificationResult`) — that's a separate seam.
- No CLI presentation change — `DriverError::Display` is already reasonable;
  this PR is about making errors *classifiable*, not *prettier*.
