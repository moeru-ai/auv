# Window and Screen OCR Click Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace fixed/main-display OCR behavior with explicit screen/display/window observation commands, shared window resolution, and migrated NetEaseMusic recipe coverage.

**Architecture:** Add shared source-resolution helpers under the macOS driver, then route screen OCR, window OCR, capture, and click commands through those helpers. Promote `listWindows` to the window candidate API, rename AX tree observation, and migrate recipes/docs after the command layer is testable.

**Tech Stack:** Rust 2024, xcap macOS capture backend, Swift helper scripts, macOS Vision OCR, JSON/text run artifacts, cargo test/fmt/check/clippy.

---

## Current Files and Responsibilities

- `src/catalog.rs`: public command ids and disturbance metadata.
- `src/driver/macos/dispatch.rs`: command operation routing.
- `src/driver/macos/types.rs`: shared macOS observation structs.
- `src/driver/macos/support/selector.rs`: app/window selection logic.
- `src/driver/macos/support/display.rs`: display enumeration and display parsing helpers.
- `src/driver/macos/support/geometry.rs`: coordinate projection and window point math.
- `src/driver/macos/support/ocr.rs`: OCR report parsing, filtering, row detection.
- `src/driver/macos/observe.rs`: current screen OCR, wait, list/observe, AX tree commands.
- `src/driver/macos/control/screen.rs`: current OCR click and row click commands.
- `src/driver/macos/control/window.rs`: current window-relative point click.
- `src/driver/macos/capture/commands.rs`: captureDisplay/captureRegion/captureWindow/listDisplays commands.
- `src/driver/macos/capture/xcap_backend.rs`: xcap display/window descriptors and capture projection.
- `src/driver/macos/tests.rs`: macOS helper unit tests.
- `src/cli.rs`: command help text.
- `recipes/macos/netease-cloud-music/play-visible-anchor.v0.json`: NetEaseMusic playback recipe to migrate.
- `recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json`: NetEaseMusic case matrix to migrate.
- `recipes/macos/netease-cloud-music/README.md`: NetEaseMusic recipe documentation.
- `docs/TERMS_AND_CONCEPTS.md`: durable vocabulary already updated by the design.
- `docs/ai/references/driver/2026-05-20-window-screen-ocr-click-design.md`: design source for this plan.

## Implementation-Time Checks

- Check whether xcap `Window::id()` is stable only for a live window session or across process restarts. Treat `native_window_id` as stronger than list order, but do not promise persistence beyond the current desktop state unless live validation proves it.
- Check whether the existing `window_ref` values from `captureWindow` are snapshot-scoped. If they are, document that `window_ref` should usually be produced by `listWindows` and consumed soon after.
- Verify the Swift window helper can report enough metadata for `isFullyContainedInDisplay`. If not, compute containment in Rust from window/display bounds.
- Confirm whether `debug.observeWindowTree` is referenced by recipes or docs. The implementation should migrate those references to `debug.observeAxTree`.

---

### Task 1: Add Window Candidate Types and Pure Resolver Tests

**Files:**
- Modify: `src/driver/macos/types.rs`
- Modify: `src/driver/macos/support/selector.rs`
- Modify: `src/driver/macos/support/mod.rs`
- Test: `src/driver/macos/tests.rs`

- [ ] **Step 1: Add failing tests for candidate ordering and no index selector**

Append tests to `src/driver/macos/tests.rs`:

```rust
#[test]
fn build_window_candidates_marks_main_and_containment() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");

  let candidates =
    build_window_candidates(&snapshot, &resolved, &displays).expect("candidates should build");

  assert_eq!(candidates.len(), 2);
  assert_eq!(candidates[0].window_ref.window_number, 42);
  assert!(candidates[0].is_main_candidate);
  assert!(candidates[0].is_fully_contained_in_display);
  assert_eq!(candidates[0].display_ref.as_deref(), Some("display_1"));
  assert_eq!(candidates[0].selection_reason, "largest-visible-normal-window");
  assert_eq!(candidates[0].candidate_index, 0);
}

#[test]
fn resolve_window_candidate_rejects_ambiguous_title() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_ambiguous_title_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let requested = WindowSelection {
    window_ref: None,
    native_window_id: None,
    title: Some("Main Window".to_string()),
  };

  let error = resolve_window_candidate(&snapshot, &resolved, &displays, &requested)
    .expect_err("ambiguous title should fail");

  assert!(error.contains("multiple window candidates matched title"));
  assert!(error.contains("debug.listWindows"));
}
```

- [ ] **Step 2: Run the failing resolver tests**

Run:

```bash
cargo test build_window_candidates_marks_main_and_containment resolve_window_candidate_rejects_ambiguous_title
```

Expected: both tests fail because `WindowCandidate`, `WindowSelection`, `build_window_candidates`, `resolve_window_candidate`, and fixtures are missing.

- [ ] **Step 3: Add window candidate structs**

Add to `src/driver/macos/types.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct WindowCandidate {
  pub(crate) candidate_index: usize,
  pub(crate) window_ref: WindowRef,
  pub(crate) native_window_id: Option<String>,
  pub(crate) display_ref: Option<String>,
  pub(crate) native_display_id: Option<String>,
  pub(crate) is_main_candidate: bool,
  pub(crate) is_fully_contained_in_display: bool,
  pub(crate) area: i64,
  pub(crate) selection_reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct WindowSelection {
  pub(crate) window_ref: Option<String>,
  pub(crate) native_window_id: Option<String>,
  pub(crate) title: Option<String>,
}
```

- [ ] **Step 4: Implement pure candidate construction and resolver**

Add to `src/driver/macos/support/selector.rs`:

```rust
pub(crate) fn build_window_candidates(
  snapshot: &ObservedWindowSnapshot,
  resolved_app: &ResolvedAppRef,
  displays: &[crate::driver::macos::capture::types::DisplayDescriptor],
) -> AuvResult<Vec<WindowCandidate>> {
  let mut windows = snapshot
    .windows
    .iter()
    .filter(|window| window_matches_resolved_app(window, resolved_app))
    .filter(|window| is_substantial_window(window))
    .collect::<Vec<_>>();

  windows.sort_by_key(|window| {
    std::cmp::Reverse((
      if window.layer == 0 { 1 } else { 0 },
      if !window.title.trim().is_empty() { 1 } else { 0 },
      window_area(window),
    ))
  });

  let max_area = windows.iter().map(|window| window_area(window)).max().unwrap_or(0);
  Ok(windows
    .into_iter()
    .enumerate()
    .map(|(candidate_index, window)| {
      let display = containing_display(&window.bounds, displays);
      WindowCandidate {
        candidate_index,
        window_ref: window.to_window_ref(),
        native_window_id: Some(window.window_number.to_string()),
        display_ref: display.map(|display| display.display_ref.clone()),
        native_display_id: display.map(|display| display.native_display_id.clone()),
        is_main_candidate: window_area(window) == max_area && window.layer == 0,
        is_fully_contained_in_display: display.is_some(),
        area: window_area(window),
        selection_reason: if window_area(window) == max_area && window.layer == 0 {
          "largest-visible-normal-window".to_string()
        } else {
          "visible-app-window".to_string()
        },
      }
    })
    .collect())
}

pub(crate) fn resolve_window_candidate(
  snapshot: &ObservedWindowSnapshot,
  resolved_app: &ResolvedAppRef,
  displays: &[crate::driver::macos::capture::types::DisplayDescriptor],
  selection: &WindowSelection,
) -> AuvResult<WindowCandidate> {
  let candidates = build_window_candidates(snapshot, resolved_app, displays)?;
  let filtered = filter_window_candidates(&candidates, selection);
  if filtered.len() == 1 {
    return Ok(filtered[0].clone());
  }
  if filtered.len() > 1 {
    return Err("multiple window candidates matched selector; inspect `debug.listWindows` and provide --window_ref or --native_window_id".to_string());
  }
  candidates
    .into_iter()
    .find(|candidate| candidate.is_main_candidate && candidate.is_fully_contained_in_display)
    .or_else(|| {
      build_window_candidates(snapshot, resolved_app, displays)
        .ok()
        .and_then(|candidates| candidates.into_iter().find(|candidate| candidate.is_fully_contained_in_display))
    })
    .ok_or_else(|| "could not resolve a fully contained visible window; inspect `debug.listWindows`".to_string())
}
```

Also add private helpers in the same file:

```rust
fn filter_window_candidates<'a>(
  candidates: &'a [WindowCandidate],
  selection: &WindowSelection,
) -> Vec<&'a WindowCandidate> {
  candidates
    .iter()
    .filter(|candidate| {
      selection.window_ref.as_ref().is_none_or(|expected| {
        expected == &candidate.window_ref.window_number.to_string()
      })
    })
    .filter(|candidate| {
      selection.native_window_id.as_ref().is_none_or(|expected| {
        candidate.native_window_id.as_ref() == Some(expected)
      })
    })
    .filter(|candidate| {
      selection.title.as_ref().is_none_or(|expected| {
        candidate.window_ref.title == *expected
      })
    })
    .collect()
}

fn containing_display<'a>(
  bounds: &ObservedRect,
  displays: &'a [crate::driver::macos::capture::types::DisplayDescriptor],
) -> Option<&'a crate::driver::macos::capture::types::DisplayDescriptor> {
  displays.iter().find(|display| {
    let display_bounds = &display.global_logical_bounds;
    bounds.x as f64 >= display_bounds.x
      && bounds.y as f64 >= display_bounds.y
      && (bounds.x + bounds.width) as f64 <= display_bounds.x + display_bounds.width
      && (bounds.y + bounds.height) as f64 <= display_bounds.y + display_bounds.height
  })
}
```

- [ ] **Step 5: Add test fixtures**

Append fixture helpers to `src/driver/macos/tests.rs`:

```rust
fn sample_display_descriptors_for_windows() -> Vec<super::capture::types::DisplayDescriptor> {
  vec![
    super::capture::types::DisplayDescriptor {
      display_ref: "display_0".to_string(),
      native_display_id: "3".to_string(),
      is_main: true,
      is_builtin: true,
      global_logical_bounds: super::capture::types::Rect { x: 0.0, y: 0.0, width: 1512.0, height: 982.0 },
      visible_logical_bounds: super::capture::types::Rect { x: 0.0, y: 0.0, width: 1512.0, height: 982.0 },
      physical_pixel_size: super::capture::types::Size { width: 3024.0, height: 1964.0 },
      scale_factor: 2.0,
      pixel_to_logical_scale: super::capture::types::Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: super::capture::types::Scale2D { x: 2.0, y: 2.0 },
      capture_backend: super::capture::types::CaptureBackend::XcapMacos,
    },
    super::capture::types::DisplayDescriptor {
      display_ref: "display_1".to_string(),
      native_display_id: "2".to_string(),
      is_main: false,
      is_builtin: false,
      global_logical_bounds: super::capture::types::Rect { x: 1512.0, y: 0.0, width: 1643.0, height: 1053.0 },
      visible_logical_bounds: super::capture::types::Rect { x: 1512.0, y: 0.0, width: 1643.0, height: 1053.0 },
      physical_pixel_size: super::capture::types::Size { width: 3286.0, height: 2106.0 },
      scale_factor: 2.0,
      pixel_to_logical_scale: super::capture::types::Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: super::capture::types::Scale2D { x: 2.0, y: 2.0 },
      capture_backend: super::capture::types::CaptureBackend::XcapMacos,
    },
  ]
}

fn sample_multi_window_snapshot() -> super::ObservedWindowSnapshot {
  super::ObservedWindowSnapshot {
    frontmost_app_name: "ExampleMusic".to_string(),
    frontmost_app_bundle_id: "com.example.music".to_string(),
    frontmost_window_title: "Main Window".to_string(),
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    windows: vec![
      super::ObservedWindow {
        window_number: 42,
        app_name: "ExampleMusic".to_string(),
        owner_pid: 100,
        owner_bundle_id: "com.example.music".to_string(),
        layer: 0,
        title: "Main Window".to_string(),
        bounds: super::ObservedRect { x: 1600, y: 50, width: 1200, height: 800 },
      },
      super::ObservedWindow {
        window_number: 43,
        app_name: "ExampleMusic".to_string(),
        owner_pid: 100,
        owner_bundle_id: "com.example.music".to_string(),
        layer: 0,
        title: "Mini Player".to_string(),
        bounds: super::ObservedRect { x: 100, y: 100, width: 320, height: 180 },
      },
    ],
  }
}

fn sample_ambiguous_title_window_snapshot() -> super::ObservedWindowSnapshot {
  let mut snapshot = sample_multi_window_snapshot();
  snapshot.windows[1].title = "Main Window".to_string();
  snapshot.windows[1].bounds = super::ObservedRect { x: 100, y: 100, width: 900, height: 600 };
  snapshot
}
```

- [ ] **Step 6: Run resolver tests**

Run:

```bash
cargo test build_window_candidates_marks_main_and_containment resolve_window_candidate_rejects_ambiguous_title
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add src/driver/macos/types.rs src/driver/macos/support/selector.rs src/driver/macos/support/mod.rs src/driver/macos/tests.rs
git commit -m "feat(macos): add window candidate resolver"
```

---

### Task 2: Replace observeWindows with listWindows Artifacts

**Files:**
- Modify: `src/catalog.rs`
- Modify: `src/driver/macos/dispatch.rs`
- Modify: `src/driver/macos/observe.rs`
- Test: `src/driver/macos/tests.rs`

- [ ] **Step 1: Write command catalog test**

Modify the command catalog tests in `src/catalog.rs`:

```rust
#[test]
fn command_catalog_resolves_window_listing_commands() {
  let catalog = default_command_catalog();
  assert!(catalog.resolve("debug.listDisplays").is_some());
  assert!(catalog.resolve("debug.listWindows").is_some());
  assert!(catalog.resolve("debug.observeWindows").is_none());
}
```

- [ ] **Step 2: Run the catalog test**

Run:

```bash
cargo test command_catalog_resolves_window_listing_commands
```

Expected: FAIL because `debug.listWindows` is not registered or `debug.observeWindows` still exists.

- [ ] **Step 3: Update catalog and dispatch**

In `src/catalog.rs`, replace the `debug.observeWindows` command spec with:

```rust
CommandSpec {
  id: "debug.listWindows",
  summary: "List visible macOS window candidates using the normalized AUV window selector model.",
  driver_id: "macos.observe",
  operation: "list_windows",
  disturbance_classes: NONE,
  max_disturbance: DisturbanceClass::None,
},
```

In `src/driver/macos/dispatch.rs`, route:

```rust
"list_windows" => list_windows(call),
```

and remove the `observe_windows` route.

- [ ] **Step 4: Rename command function and add JSON artifact**

Rename `observe_windows` to `list_windows` in `src/driver/macos/observe.rs`.
Keep the human-readable text report, and add JSON rendering:

```rust
let json = serde_json::to_string_pretty(&rendered_candidates).map_err(|error| {
  format!("failed to encode window candidate list JSON: {error}")
})? + "\n";
let json_artifact = build_text_artifact(
  "window-list",
  "json",
  "window-list",
  json,
  "Machine-readable macOS window candidate list.",
)?;
```

Return both artifacts:

```rust
artifacts: vec![json_artifact, text_artifact],
```

Use the existing text report as the second artifact so manual debugging remains easy.

- [ ] **Step 5: Add a pure JSON shape test**

Add to `src/driver/macos/tests.rs`:

```rust
#[test]
fn window_candidate_json_contains_stable_selector_fields() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let candidates =
    build_window_candidates(&snapshot, &resolved, &displays).expect("candidates should build");

  let json = serde_json::to_value(&candidates[0]).expect("candidate should encode");

  assert_eq!(json["candidate_index"], 0);
  assert_eq!(json["window_ref"]["window_number"], 42);
  assert_eq!(json["native_window_id"], "42");
  assert_eq!(json["display_ref"], "display_1");
  assert_eq!(json["native_display_id"], "2");
}
```

If `WindowCandidate` is not serializable, derive `serde::Serialize` on the candidate types and any nested structs needed by the JSON artifact.

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test command_catalog_resolves_window_listing_commands window_candidate_json_contains_stable_selector_fields
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add src/catalog.rs src/driver/macos/dispatch.rs src/driver/macos/observe.rs src/driver/macos/types.rs src/driver/macos/tests.rs
git commit -m "feat(macos): list window candidates"
```

---

### Task 3: Share Window Resolver in captureWindow and clickWindowPoint

**Files:**
- Modify: `src/driver/macos/capture/commands.rs`
- Modify: `src/driver/macos/control/window.rs`
- Modify: `src/driver/macos/support/call.rs`
- Test: `src/driver/macos/tests.rs`

- [ ] **Step 1: Add selection parser tests**

Add to `src/driver/macos/tests.rs`:

```rust
#[test]
fn parse_window_selection_accepts_ref_native_id_and_title() {
  let call = build_call([
    ("window_ref", "42"),
    ("native_window_id", "42"),
    ("title", "Main Window"),
  ]);

  let selection = parse_window_selection(&call).expect("selection should parse");

  assert_eq!(selection.window_ref.as_deref(), Some("42"));
  assert_eq!(selection.native_window_id.as_deref(), Some("42"));
  assert_eq!(selection.title.as_deref(), Some("Main Window"));
}

#[test]
fn parse_window_selection_rejects_window_index() {
  let call = build_call([("window_index", "1")]);

  let error = parse_window_selection(&call).expect_err("window_index should be rejected");

  assert!(error.contains("--window_index is not supported"));
}
```

- [ ] **Step 2: Run selection parser tests**

Run:

```bash
cargo test parse_window_selection_accepts_ref_native_id_and_title parse_window_selection_rejects_window_index
```

Expected: FAIL because `parse_window_selection` is missing.

- [ ] **Step 3: Implement parser**

Add to `src/driver/macos/support/call.rs`:

```rust
pub(crate) fn parse_window_selection(call: &DriverCall) -> AuvResult<WindowSelection> {
  if call.inputs.contains_key("window_index") {
    return Err("--window_index is not supported because window candidate order is not stable; use --window_ref, --native_window_id, or --title".to_string());
  }
  Ok(WindowSelection {
    window_ref: optional_non_empty_string(call, "window_ref"),
    native_window_id: optional_non_empty_string(call, "native_window_id"),
    title: optional_non_empty_string(call, "title"),
  })
}
```

- [ ] **Step 4: Update captureWindow selector names**

In `src/driver/macos/capture/commands.rs`, remove `window_id`, `window_title`, `window_index`, and `prefer_main_window` parsing from `capture_window`. Parse:

```rust
let selection = parse_window_selection(call)?;
```

Use the shared resolver:

```rust
let target_app = call.target.application_id.clone().unwrap_or_default();
let selector = parse_app_selector(&target_app)?;
let observed = crate::driver::macos::observe::observe_windows_snapshot(128, &target_app)?;
let resolved_app = resolve_app_ref(&observed, &selector)?;
let selected_candidate = resolve_window_candidate(&observed, &resolved_app, &displays, &selection)?;
```

Then refresh the xcap window by `native_window_id` or `window_ref.window_number` and keep the existing capture contract flow.

- [ ] **Step 5: Update clickWindowPoint to the shared resolver**

In `src/driver/macos/control/window.rs`, replace the local `resolve_window_ref` path with `parse_window_selection` and `resolve_window_candidate`. Use the candidate's `window_ref` for `resolve_window_point`.

The report must include:

```rust
format!("candidateIndex={}", selected.candidate_index),
format!("selectionReason={}", selected.selection_reason),
format!("isFullyContainedInDisplay={}", selected.is_fully_contained_in_display),
```

- [ ] **Step 6: Run focused tests**

Run:

```bash
cargo test parse_window_selection_accepts_ref_native_id_and_title parse_window_selection_rejects_window_index resolve_window_point_supports_relative_mode
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add src/driver/macos/capture/commands.rs src/driver/macos/control/window.rs src/driver/macos/support/call.rs src/driver/macos/tests.rs
git commit -m "refactor(macos): share window resolver"
```

---

### Task 4: Add Observation Source Helpers for Screen and Window OCR

**Files:**
- Create: `src/driver/macos/support/observation.rs`
- Modify: `src/driver/macos/support/mod.rs`
- Modify: `src/driver/macos/observe.rs`
- Modify: `src/driver/macos/control/screen.rs`
- Test: `src/driver/macos/tests.rs`

- [ ] **Step 1: Write source selection tests**

Add to `src/driver/macos/tests.rs`:

```rust
#[test]
fn resolve_screen_capture_source_prefers_explicit_display() {
  let displays = sample_display_descriptors_for_windows();
  let selection = DisplaySelection {
    display_ref: Some("display_1".to_string()),
    native_display_id: None,
    main: false,
  };

  let source = resolve_screen_capture_source(&displays, Some(&selection), None)
    .expect("source should resolve");

  assert_eq!(source.display_ref, "display_1");
  assert_eq!(source.selection_reason, "explicit-display-ref");
}

#[test]
fn resolve_screen_capture_source_uses_target_window_display() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let candidate =
    resolve_window_candidate(&snapshot, &resolved, &displays, &WindowSelection::default())
      .expect("candidate should resolve");

  let source = resolve_screen_capture_source(&displays, None, Some(&candidate))
    .expect("source should resolve");

  assert_eq!(source.display_ref, "display_1");
  assert_eq!(source.selection_reason, "target-window-display");
}
```

- [ ] **Step 2: Run source selection tests**

Run:

```bash
cargo test resolve_screen_capture_source_prefers_explicit_display resolve_screen_capture_source_uses_target_window_display
```

Expected: FAIL because the source helpers are missing.

- [ ] **Step 3: Create observation helper module**

Create `src/driver/macos/support/observation.rs`:

```rust
use super::super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DisplaySelection {
  pub(crate) display_ref: Option<String>,
  pub(crate) native_display_id: Option<String>,
  pub(crate) main: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedScreenCaptureSource {
  pub(crate) display_ref: String,
  pub(crate) native_display_id: String,
  pub(crate) selection_reason: String,
}

pub(crate) fn parse_display_selection(call: &DriverCall) -> AuvResult<Option<DisplaySelection>> {
  let display_ref = optional_non_empty_string(call, "display_ref");
  let native_display_id = optional_non_empty_string(call, "native_display_id")
    .or_else(|| optional_non_empty_string(call, "display_id"));
  let main = optional_bool(call, "main")?.unwrap_or(false);
  if display_ref.is_none() && native_display_id.is_none() && !main {
    return Ok(None);
  }
  Ok(Some(DisplaySelection {
    display_ref,
    native_display_id,
    main,
  }))
}

pub(crate) fn resolve_screen_capture_source(
  displays: &[crate::driver::macos::capture::types::DisplayDescriptor],
  display_selection: Option<&DisplaySelection>,
  target_window: Option<&WindowCandidate>,
) -> AuvResult<ResolvedScreenCaptureSource> {
  if let Some(selection) = display_selection {
    if let Some(display_ref) = selection.display_ref.as_deref() {
      let display = displays.iter().find(|display| display.display_ref == display_ref)
        .ok_or_else(|| format!("display selector --display_ref {display_ref} did not match current displays"))?;
      return Ok(ResolvedScreenCaptureSource {
        display_ref: display.display_ref.clone(),
        native_display_id: display.native_display_id.clone(),
        selection_reason: "explicit-display-ref".to_string(),
      });
    }
    if let Some(native_display_id) = selection.native_display_id.as_deref() {
      let display = displays.iter().find(|display| display.native_display_id == native_display_id)
        .ok_or_else(|| format!("display selector --native_display_id {native_display_id} did not match current displays"))?;
      return Ok(ResolvedScreenCaptureSource {
        display_ref: display.display_ref.clone(),
        native_display_id: display.native_display_id.clone(),
        selection_reason: "explicit-native-display-id".to_string(),
      });
    }
    if selection.main {
      let display = displays.iter().find(|display| display.is_main).or_else(|| displays.first())
        .ok_or_else(|| "display list is empty".to_string())?;
      return Ok(ResolvedScreenCaptureSource {
        display_ref: display.display_ref.clone(),
        native_display_id: display.native_display_id.clone(),
        selection_reason: "explicit-main-display".to_string(),
      });
    }
  }

  if let Some(candidate) = target_window
    && let (Some(display_ref), Some(native_display_id)) =
      (candidate.display_ref.as_ref(), candidate.native_display_id.as_ref())
  {
    return Ok(ResolvedScreenCaptureSource {
      display_ref: display_ref.clone(),
      native_display_id: native_display_id.clone(),
      selection_reason: "target-window-display".to_string(),
    });
  }

  let display = displays.iter().find(|display| display.is_main).or_else(|| displays.first())
    .ok_or_else(|| "display list is empty".to_string())?;
  Ok(ResolvedScreenCaptureSource {
    display_ref: display.display_ref.clone(),
    native_display_id: display.native_display_id.clone(),
    selection_reason: "main-display-fallback".to_string(),
  })
}
```

Export it from `src/driver/macos/support/mod.rs`:

```rust
mod observation;
pub(crate) use self::observation::*;
```

- [ ] **Step 4: Run source selection tests**

Run:

```bash
cargo test resolve_screen_capture_source_prefers_explicit_display resolve_screen_capture_source_uses_target_window_display
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add src/driver/macos/support/observation.rs src/driver/macos/support/mod.rs src/driver/macos/tests.rs
git commit -m "feat(macos): resolve observation sources"
```

---

### Task 5: Refactor OCR Text/Row Pipeline for Reuse

**Files:**
- Create: `src/driver/macos/support/ocr_commands.rs`
- Modify: `src/driver/macos/support/mod.rs`
- Modify: `src/driver/macos/observe.rs`
- Modify: `src/driver/macos/control/screen.rs`
- Test: `src/driver/macos/tests.rs`

- [ ] **Step 1: Write report rendering tests**

Add to `src/driver/macos/tests.rs`:

```rust
#[test]
fn render_text_match_json_records_scope_and_point() {
  let report = TextMatchCommandReport {
    scope: "window".to_string(),
    capture_source: "window_42".to_string(),
    query: "Primary Track".to_string(),
    match_count: 1,
    filtered_match_count: 1,
    region: Some(super::ObservedRect { x: 10, y: 20, width: 300, height: 200 }),
    best_match_bounds: Some(super::ObservedRect { x: 40, y: 60, width: 120, height: 24 }),
    screenshot_point: Some((100.0, 72.0)),
    logical_point: Some((1650.0, 112.0)),
  };

  let json = render_text_match_command_json(&report).expect("json should render");

  assert!(json.contains("\"scope\": \"window\""));
  assert!(json.contains("\"capture_source\": \"window_42\""));
  assert!(json.contains("\"logical_point\""));
}
```

- [ ] **Step 2: Run report rendering test**

Run:

```bash
cargo test render_text_match_json_records_scope_and_point
```

Expected: FAIL because `TextMatchCommandReport` and render helper are missing.

- [ ] **Step 3: Add shared report structs and renderers**

Create `src/driver/macos/support/ocr_commands.rs`:

```rust
use super::super::*;

#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct TextMatchCommandReport {
  pub(crate) scope: String,
  pub(crate) capture_source: String,
  pub(crate) query: String,
  pub(crate) match_count: usize,
  pub(crate) filtered_match_count: usize,
  pub(crate) region: Option<ObservedRect>,
  pub(crate) best_match_bounds: Option<ObservedRect>,
  pub(crate) screenshot_point: Option<(f64, f64)>,
  pub(crate) logical_point: Option<(f64, f64)>,
}

pub(crate) fn render_text_match_command_json(report: &TextMatchCommandReport) -> AuvResult<String> {
  serde_json::to_string_pretty(report)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| format!("failed to encode text match command report JSON: {error}"))
}
```

Derive `serde::Serialize` for `ObservedRect` in `src/driver/macos/types.rs`:

```rust
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ObservedRect {
  ...
}
```

Export the new module from `src/driver/macos/support/mod.rs`.

- [ ] **Step 4: Move duplicated OCR command flow into helper functions**

In `src/driver/macos/support/ocr_commands.rs`, add helpers:

```rust
pub(crate) struct CapturedObservation {
  pub(crate) scope: String,
  pub(crate) capture_source: String,
  pub(crate) screenshot_path: std::path::PathBuf,
  pub(crate) capture_contract: crate::driver::macos::capture::types::CaptureContract,
  pub(crate) dimensions: ScreenshotDimensions,
}

pub(crate) fn run_text_match_on_capture(
  call: &DriverCall,
  capture: &CapturedObservation,
  query: &str,
) -> AuvResult<(OcrTextSnapshot, Vec<OcrTextMatch>, Option<TextMatchCommandReport>)> {
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?.unwrap_or(64).clamp(1, 256);
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let region = parse_ocr_region_constraint(call, capture.dimensions.width, capture.dimensions.height)?;
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    capture.screenshot_path.as_path(),
    query,
    exact,
    case_sensitive,
    max_observations,
    region.as_ref(),
  ))?;
  let snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let filtered = filter_ocr_matches(&snapshot.matches, min_confidence, region.as_ref())
    .into_iter()
    .cloned()
    .collect::<Vec<_>>();
  let report = filtered.first().map(|best| {
    let (sx, sy) = ocr_match_center(best);
    let logical = crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
      &capture.capture_contract,
      sx,
      sy,
    )
    .ok();
    TextMatchCommandReport {
      scope: capture.scope.clone(),
      capture_source: capture.capture_source.clone(),
      query: query.to_string(),
      match_count: snapshot.matches.len(),
      filtered_match_count: filtered.len(),
      region: region.clone(),
      best_match_bounds: Some(best.bounds.clone()),
      screenshot_point: Some((sx, sy)),
      logical_point: logical,
    }
  });
  Ok((snapshot, filtered, report))
}
```

- [ ] **Step 5: Run existing OCR tests**

Run:

```bash
cargo test parse_ocr_text_snapshot_parses_matches filter_ocr_matches_applies_confidence_and_region group_ocr_matches_into_rows_merges_nearby_vertical_observations render_text_match_json_records_scope_and_point
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add src/driver/macos/support/ocr_commands.rs src/driver/macos/support/mod.rs src/driver/macos/types.rs src/driver/macos/tests.rs
git commit -m "refactor(macos): share ocr command reports"
```

---

### Task 6: Migrate Screen OCR Commands to Target Display Selection

**Files:**
- Modify: `src/driver/macos/observe.rs`
- Modify: `src/driver/macos/control/screen.rs`
- Modify: `src/driver/macos/capture/xcap_backend.rs`
- Test: `src/driver/macos/tests.rs`

- [ ] **Step 1: Add tests for display selector parser aliases**

Add to `src/driver/macos/tests.rs`:

```rust
#[test]
fn parse_display_selection_accepts_native_display_id() {
  let call = build_call([("native_display_id", "2")]);
  let selection = parse_display_selection(&call)
    .expect("selection should parse")
    .expect("selection should exist");

  assert_eq!(selection.native_display_id.as_deref(), Some("2"));
  assert!(!selection.main);
}
```

- [ ] **Step 2: Run parser test**

Run:

```bash
cargo test parse_display_selection_accepts_native_display_id
```

Expected: PASS after Task 4.

- [ ] **Step 3: Add display capture helper by source**

In `src/driver/macos/capture/xcap_backend.rs`, add a helper that captures an explicit display ref/native id without assuming main display:

```rust
pub(crate) fn capture_display_ref_to_path(
  label: &str,
  display_ref: &str,
) -> AuvResult<(std::path::PathBuf, CaptureContract)> {
  let monitors = xcap::Monitor::all().map_err(|error| {
    format!("{}: failed to enumerate displays before capture: {error}", capture_error::BACKEND_FAILED)
  })?;
  let displays = descriptors_from_monitors(&monitors)?;
  let display_index = displays
    .iter()
    .position(|display| display.display_ref == display_ref)
    .ok_or_else(|| format!("{}: stale display ref {display_ref}", capture_error::STALE_DISPLAY_REF))?;
  capture_display_index_to_path(label, &monitors, &displays, display_index)
}
```

Extract the existing save/contract code from `capture_main_display_to_path` into `capture_display_index_to_path` so both helpers use one implementation.

- [ ] **Step 4: Update screen find/wait/click/row commands**

Replace calls to:

```rust
capture_main_display_to_path(&label)
```

with source resolution:

```rust
let displays = crate::driver::macos::capture::xcap_backend::list_displays()?;
let display_selection = parse_display_selection(call)?;
let target_window = resolve_optional_target_window_candidate(call, &displays)?;
let source = resolve_screen_capture_source(&displays, display_selection.as_ref(), target_window.as_ref())?;
let (screenshot_path, capture_contract) =
  crate::driver::macos::capture::xcap_backend::capture_display_ref_to_path(&label, &source.display_ref)?;
```

Apply this in:

- `find_screen_text`
- `wait_for_screen_text`
- `find_screen_rows`
- `wait_for_screen_rows`
- `click_screen_text`
- `click_screen_row`

Add notes:

```rust
format!("scope=screen"),
format!("displayRef={}", source.display_ref),
format!("displaySelectionReason={}", source.selection_reason),
```

- [ ] **Step 5: Run screen OCR tests**

Run:

```bash
cargo test parse_display_selection_accepts_native_display_id
cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add src/driver/macos/observe.rs src/driver/macos/control/screen.rs src/driver/macos/capture/xcap_backend.rs src/driver/macos/tests.rs
git commit -m "feat(macos): select display for screen ocr"
```

---

### Task 7: Add Window Text and Row Commands

**Files:**
- Modify: `src/catalog.rs`
- Modify: `src/driver/macos/dispatch.rs`
- Create: `src/driver/macos/control/window_ocr.rs`
- Modify: `src/driver/macos/control/mod.rs`
- Modify: `src/driver/macos/observe.rs`
- Test: `src/catalog.rs`
- Test: `src/driver/macos/tests.rs`

- [ ] **Step 1: Add catalog tests**

Add to `src/catalog.rs` tests:

```rust
#[test]
fn command_catalog_resolves_window_ocr_commands() {
  let catalog = default_command_catalog();
  for command_id in [
    "debug.findWindowText",
    "debug.waitForWindowText",
    "debug.clickWindowText",
    "debug.findWindowRows",
    "debug.waitForWindowRows",
    "debug.clickWindowRow",
  ] {
    assert!(catalog.resolve(command_id).is_some(), "missing {command_id}");
  }
}
```

- [ ] **Step 2: Run catalog test**

Run:

```bash
cargo test command_catalog_resolves_window_ocr_commands
```

Expected: FAIL because commands are not registered.

- [ ] **Step 3: Register commands**

Add command specs in `src/catalog.rs`:

```rust
CommandSpec {
  id: "debug.findWindowText",
  summary: "Capture a resolved window and locate OCR text anchors in window pixel space.",
  driver_id: "macos.observe",
  operation: "find_window_text",
  disturbance_classes: NONE,
  max_disturbance: DisturbanceClass::None,
},
CommandSpec {
  id: "debug.waitForWindowText",
  summary: "Poll resolved-window OCR until a text anchor appears or the timeout expires.",
  driver_id: "macos.observe",
  operation: "wait_for_window_text",
  disturbance_classes: NONE,
  max_disturbance: DisturbanceClass::None,
},
CommandSpec {
  id: "debug.clickWindowText",
  summary: "Capture a resolved window, resolve an OCR text anchor, and click its projected logical point.",
  driver_id: "macos.observe",
  operation: "click_window_text",
  disturbance_classes: POINTER_WITH_FOREGROUND,
  max_disturbance: DisturbanceClass::Pointer,
},
CommandSpec {
  id: "debug.findWindowRows",
  summary: "Detect visible OCR row bands inside a resolved window.",
  driver_id: "macos.observe",
  operation: "find_window_rows",
  disturbance_classes: NONE,
  max_disturbance: DisturbanceClass::None,
},
CommandSpec {
  id: "debug.waitForWindowRows",
  summary: "Poll resolved-window row detection until enough rows appear or the timeout expires.",
  driver_id: "macos.observe",
  operation: "wait_for_window_rows",
  disturbance_classes: NONE,
  max_disturbance: DisturbanceClass::None,
},
CommandSpec {
  id: "debug.clickWindowRow",
  summary: "Capture a resolved window, detect visible rows, and click a row-derived projected logical point.",
  driver_id: "macos.observe",
  operation: "click_window_row",
  disturbance_classes: POINTER_WITH_FOREGROUND,
  max_disturbance: DisturbanceClass::Pointer,
},
```

- [ ] **Step 4: Implement command routing**

In `src/driver/macos/dispatch.rs`, route:

```rust
"find_window_text" => find_window_text(call),
"wait_for_window_text" => wait_for_window_text(call),
"click_window_text" => click_window_text(call),
"find_window_rows" => find_window_rows(call),
"wait_for_window_rows" => wait_for_window_rows(call),
"click_window_row" => click_window_row(call),
```

- [ ] **Step 5: Add window OCR module**

Create `src/driver/macos/control/window_ocr.rs` with wrappers that mirror screen logic but call a shared window capture helper:

```rust
use super::super::*;
use super::common::{ClickPointCallOptions, build_click_point_call, resolve_click_interval_ms};
use super::pointer::click_point;

pub(crate) fn click_window_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("window-text-click-{}", sanitize_file_component(&query));
  let capture = capture_resolved_window_observation(call, &label)?;
  let (_snapshot, filtered, report) = run_text_match_on_capture(call, &capture, &query)?;
  let matched = filtered.first().ok_or_else(|| {
    format!("no filtered OCR text match for query {query} inside resolved window; inspect `debug.findWindowText`")
  })?;
  let (sx, sy) = ocr_match_center(matched);
  let (logical_x, logical_y) =
    crate::driver::macos::capture::xcap_backend::project_capture_pixel_to_global_logical(
      &capture.capture_contract,
      sx,
      sy,
    )?;
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let nested_call = build_click_point_call(
    &call.target,
    call.working_directory.as_path(),
    ClickPointCallOptions {
      x: logical_x,
      y: logical_y,
      button: "left",
      click_count,
      click_interval_ms: Some(click_interval_ms),
      settle_ms: Some(settle_ms),
      app: call.target.application_id.as_deref(),
    },
  );
  let _ = click_point(&nested_call)?;
  build_window_text_click_response(capture, matched, report, logical_x, logical_y, click_count, click_interval_ms, settle_ms)
}
```

Implement `find_window_text`, `wait_for_window_text`, `find_window_rows`, `wait_for_window_rows`, and `click_window_row` by reusing the same helpers and the current screen row code shape.

- [ ] **Step 6: Export window OCR commands**

In `src/driver/macos/control/mod.rs`:

```rust
mod window_ocr;
pub(crate) use self::window_ocr::{
  click_window_row, click_window_text, find_window_rows, find_window_text,
  wait_for_window_rows, wait_for_window_text,
};
```

- [ ] **Step 7: Run catalog and check**

Run:

```bash
cargo test command_catalog_resolves_window_ocr_commands
cargo check
```

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add src/catalog.rs src/driver/macos/dispatch.rs src/driver/macos/control/mod.rs src/driver/macos/control/window_ocr.rs
git commit -m "feat(macos): add window ocr commands"
```

---

### Task 8: Rename observeWindowTree to observeAxTree

**Files:**
- Modify: `src/catalog.rs`
- Modify: `src/driver/macos/dispatch.rs`
- Modify: `src/driver/macos/observe.rs`
- Modify: `src/cli.rs`
- Modify: docs/recipes references found by `rg "observeWindowTree|observe_window_tree|window-tree"`
- Test: `src/catalog.rs`

- [ ] **Step 1: Add catalog rename test**

Add to `src/catalog.rs` tests:

```rust
#[test]
fn command_catalog_renames_window_tree_to_ax_tree() {
  let catalog = default_command_catalog();
  assert!(catalog.resolve("debug.observeAxTree").is_some());
  assert!(catalog.resolve("debug.observeWindowTree").is_none());
}
```

- [ ] **Step 2: Run test**

Run:

```bash
cargo test command_catalog_renames_window_tree_to_ax_tree
```

Expected: FAIL because the command is not renamed.

- [ ] **Step 3: Rename command and operation**

In `src/catalog.rs`, change:

```rust
id: "debug.observeWindowTree",
operation: "observe_window_tree",
```

to:

```rust
id: "debug.observeAxTree",
summary: "Capture an AX tree snapshot for a target macOS app window.",
operation: "observe_ax_tree",
```

In `src/driver/macos/dispatch.rs`, route:

```rust
"observe_ax_tree" => observe_ax_tree(call),
```

Rename `observe_window_tree` function in `src/driver/macos/observe.rs` to `observe_ax_tree`. Keep the Swift helper filename for now because it is an implementation detail; change artifact labels from `window-tree` to `ax-tree`.

- [ ] **Step 4: Update references**

Run:

```bash
rg "observeWindowTree|observe_window_tree|window-tree" src docs recipes
```

Replace public docs/help references with `observeAxTree` / `observe_ax_tree` / `ax-tree` where they describe the command. Leave internal Swift filename mentions only when they refer to `observe_window_tree.swift`.

- [ ] **Step 5: Run rename tests**

Run:

```bash
cargo test command_catalog_renames_window_tree_to_ax_tree
cargo check
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add src/catalog.rs src/driver/macos/dispatch.rs src/driver/macos/observe.rs src/cli.rs docs recipes
git commit -m "refactor(macos): rename ax tree command"
```

---

### Task 9: Migrate NetEaseMusic Recipe and Docs

**Files:**
- Modify: `recipes/macos/netease-cloud-music/play-visible-anchor.v0.json`
- Modify: `recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json`
- Modify: `recipes/macos/netease-cloud-music/README.md`
- Modify: `docs/ai/references/apps/netease-music/2026-05-19-netease-cloud-music-fixed-layout-baseline.md`

- [ ] **Step 1: Update recipe inputs**

Remove fixed global coordinates from `play-visible-anchor.v0.json`:

```json
"search_click_x": "...",
"search_click_y": "...",
"result_click_x": "...",
"result_click_y": "..."
```

Add window-relative and region inputs:

```json
"search_relative_x": { "type": "number", "default": 0.31 },
"search_relative_y": { "type": "number", "default": 0.06 },
"result_region_left_ratio": { "type": "number", "default": 0.12 },
"result_region_top_ratio": { "type": "number", "default": 0.14 },
"result_region_right_ratio": { "type": "number", "default": 0.86 },
"result_region_bottom_ratio": { "type": "number", "default": 0.78 }
```

- [ ] **Step 2: Update recipe steps**

Change `click-search-box` to:

```json
{
  "id": "click-search-box",
  "command_id": "debug.clickWindowPoint",
  "disturbance": {
    "classes": ["foreground_app", "pointer"],
    "max": "pointer"
  },
  "args": {
    "target": "${app_id}",
    "relative_x": "${search_relative_x}",
    "relative_y": "${search_relative_y}",
    "click_count": 1,
    "click_interval_ms": "${click_interval_ms}",
    "settle_ms": "${search_click_settle_ms}"
  }
}
```

Insert `wait-for-search-result-title` before artist verification:

```json
{
  "id": "wait-for-search-result-title",
  "command_id": "debug.waitForWindowText",
  "disturbance": {
    "classes": ["none"],
    "max": "none"
  },
  "args": {
    "target": "${app_id}",
    "query": "${result_title}",
    "timeout_ms": 3500,
    "poll_interval_ms": 250,
    "min_confidence": "${result_min_confidence}",
    "max_observations": "${max_observations}",
    "region_left_ratio": "${result_region_left_ratio}",
    "region_top_ratio": "${result_region_top_ratio}",
    "region_right_ratio": "${result_region_right_ratio}",
    "region_bottom_ratio": "${result_region_bottom_ratio}"
  },
  "expect": {
    "output_must_contain": ["${result_title}"],
    "output_must_not_contain": ["Timed out"]
  }
}
```

Change `double-click-result` to `debug.clickWindowText`:

```json
{
  "id": "double-click-result",
  "command_id": "debug.clickWindowText",
  "disturbance": {
    "classes": ["foreground_app", "pointer"],
    "max": "pointer"
  },
  "args": {
    "target": "${app_id}",
    "query": "${result_title}",
    "min_confidence": "${result_min_confidence}",
    "max_observations": "${max_observations}",
    "region_left_ratio": "${result_region_left_ratio}",
    "region_top_ratio": "${result_region_top_ratio}",
    "region_right_ratio": "${result_region_right_ratio}",
    "region_bottom_ratio": "${result_region_bottom_ratio}",
    "click_count": 2,
    "click_interval_ms": "${click_interval_ms}",
    "settle_ms": "${activation_settle_ms}"
  }
}
```

- [ ] **Step 3: Update case matrix**

Set status to a revalidation state:

```json
"status": "needs-revalidation"
```

Remove old coordinate inputs and add:

```json
"search_relative_x": "0.31",
"search_relative_y": "0.06"
```

- [ ] **Step 4: Update docs**

In `recipes/macos/netease-cloud-music/README.md`, replace fixed coordinate text with:

```text
The recipe uses window-relative search-box focus and window-scoped OCR result activation. It should survive window movement across displays as long as the target window can be resolved by `debug.listWindows` and remains single-display contained.
```

In `docs/ai/references/apps/netease-music/2026-05-19-netease-cloud-music-fixed-layout-baseline.md`, add a note at the top:

```text
This fixed-layout baseline has been superseded by the window-scoped OCR design and should not be treated as validated after the 2026-05-20 migration.
```

- [ ] **Step 5: Run dry-run validation**

Run:

```bash
cargo run --quiet -- skill run macos.netease_cloud_music.play_visible_anchor.v0 --dry-run
cargo run --quiet -- skill cases run macos.netease_cloud_music.play_visible_anchor.v0 --case aurora-cure-for-me-fixed-layout --dry-run
```

Expected: both dry runs complete and show `debug.clickWindowPoint`, `debug.waitForWindowText`, and `debug.clickWindowText`.

- [ ] **Step 6: Commit**

Run:

```bash
git add recipes/macos/netease-cloud-music/play-visible-anchor.v0.json recipes/macos/netease-cloud-music/play-visible-anchor.cases.v0.json recipes/macos/netease-cloud-music/README.md docs/ai/references/apps/netease-music/2026-05-19-netease-cloud-music-fixed-layout-baseline.md
git commit -m "chore(netease): migrate recipe to window ocr"
```

---

### Task 10: Full Verification and Live PoC Checks

**Files:**
- Modify only if verification exposes a bug in files touched by prior tasks.

- [ ] **Step 1: Run formatting and static checks**

Run:

```bash
cargo fmt --check
cargo check
cargo clippy --all-targets --all-features
```

Expected: all pass.

- [ ] **Step 2: Run unit tests**

Run:

```bash
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Run command listing smoke checks**

Run:

```bash
cargo run --quiet -- list-commands
```

Expected output contains:

```text
debug.listWindows
debug.findWindowText
debug.waitForWindowText
debug.clickWindowText
debug.findWindowRows
debug.waitForWindowRows
debug.clickWindowRow
debug.observeAxTree
```

Expected output does not contain:

```text
debug.observeWindows
debug.observeAxTree
```

- [ ] **Step 4: Run live list/capture checks**

Run:

```bash
cargo run --quiet -- invoke debug.listDisplays
cargo run --quiet -- invoke debug.listWindows --target com.netease.163music
cargo run --quiet -- invoke debug.captureWindow --target com.netease.163music --label netease-window-post-migration
```

Expected:

- `listDisplays` writes a display-list JSON artifact.
- `listWindows` writes JSON and text artifacts.
- `captureWindow` captures the intended NetEaseMusic window or fails with a useful ambiguity/single-display message.

- [ ] **Step 5: Run live window OCR checks**

With NetEaseMusic open and visible, run:

```bash
cargo run --quiet -- invoke debug.findWindowText \
  --target com.netease.163music \
  --query "网易云音乐" \
  --max_observations 256 \
  --min_confidence 0.3
```

Expected: command completes and writes screenshot, JSON, and text artifacts. If OCR does not find the text, the command still reports the resolved window and filtered match count cleanly.

- [ ] **Step 6: Run migrated recipe checks**

Run:

```bash
cargo run --quiet -- skill run macos.netease_cloud_music.play_visible_anchor.v0 --dry-run
cargo run --quiet -- skill run macos.netease_cloud_music.play_visible_anchor.v0
```

Expected dry run: command sequence uses window-scoped commands.

Expected live run: either completes with bottom-player verification or fails at a window/text/row step with an inspectable artifact that shows the next region/selector adjustment needed.

- [ ] **Step 7: Final whitespace check**

Run:

```bash
git diff --check
```

Expected: no whitespace errors.

- [ ] **Step 8: Commit verification fixes**

If verification required fixes, run:

```bash
git add src recipes docs
git commit -m "fix(macos): stabilize window ocr verification"
```

If no fixes were required, do not create an empty commit.

---

## Plan Self-Review

- Spec coverage: the plan covers terminology docs, `listWindows`, shared window resolver, screen display selection, full window text/row command family, AX tree rename, recipe migration, artifacts, error handling, and verification.
- Deferred spec item: recipe-level structured binding remains outside this plan and is not needed for window-scoped OCR click commands.
- Main implementation risk: `captureWindow` currently uses xcap window descriptors while `clickWindowPoint` uses CGWindow observation structs. The resolver task must reconcile native ids and display containment across those two sources without relying on candidate order.
- API spelling check: the recipe migration uses `debug.waitForWindowText`, matching the catalog id in this plan.
