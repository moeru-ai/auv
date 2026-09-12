# Minecraft 3DGS → spatial-memory lane handoff (2026-07-27)

Records what landed on `research/3dgs-restore-lane`, what it does and does not
prove, and which gaps remain with their unlock conditions.

Branch: `research/3dgs-restore-lane`, based on `main` @ `c4aa1770`.
Local only, no upstream, no PR. Owner asked to keep it local until results exist.

## Framing correction

This lane is **not** about producing a trained 3DGS scene. The goal is a
**paradigm for agent spatial memory** that AUV can carry into apps that expose
no ground truth. Minecraft is the answer-key gym, as
[`2026-06-14-3d-minecraft-spatial-skill.md`](2026-06-14-3d-minecraft-spatial-skill.md)
§8 already stated: with a Fabric mod, raycast plus depth is a cheaper and exact
occlusion signal, so 3DGS is not needed for occlusion *in modded MC*. Its real
role is to incubate a vision-only capability for later deployment.

Consequence: 3DGS is one possible **backend** behind a viewpoint-conditioned
query, not the deliverable. Layer ladder is in
[`../../scan/2026-07-05-surface-slam-direction.md`](../../scan/2026-07-05-surface-slam-direction.md).

## Landed (committed)

| Commit | Content |
| --- | --- |
| `72f39fa6` | Restore 10844 lines of retired 3DGS domain code (13 modules, fixtures, `tests/training_result_holdout.rs`) onto post-#130 main |
| `82796c8f` | Emit seed point cloud from raycast hits |
| `acc718a7` | OpenSplat trainer backend in launch preparation |
| `db2554a0` | Trainer backend evidence for Apple Silicon |
| `509fc0ed` | Brush trainer backend in launch preparation |
| `fc5648fc` | Spatial-memory reacquisition direction note |
| `247e9185` | `reacquisition` module: viewpoint-conditioned query, 6 tests |
| `c21c892f` | Deferral markers on `reacquisition.rs` + the rustfmt repair `247e9185` shipped unformatted |
| `13216084` | Slice 1: correct the occlusion deferral rationale |
| `ef43bd92` | Slice 2: `populateNearbyBlocks` in the Fabric mod + writer-shape regression test |

Restore drift was mechanical: nine `auv_tracing_driver::now_millis()` call sites
became the crate's existing `crate::now_millis()`. `artifact_roles.rs` was
**deliberately not restored** — `TELEMETRY_SAMPLE_ARTIFACT_ROLE` no longer
exists, `dataset.rs` now owns the bundle role constants, and the restored set
references zero constants from it.

Deliberately excluded reconnection surface: `run_read.rs` 3DGS sections,
`inspect/render.rs`, all CLI wiring. `tests/feature_boundary.rs` therefore still
passes — the retired contract target was not revived.

## Working tree

Clean. The three items that were uncommitted at first handoff all landed:
the rustfmt repair and five deferral markers in `c21c892f`, and the
`TODO(reacquisition-occlusion)` rationale correction in `13216084`.

The earlier "fmt passed" claim for `247e9185` was wrong — it rested on a
terminal that returned exit 0 with empty output. Note for future checks:
`cargo fmt --check` in this repo prints `chain_width cannot have a value that
exceeds max_width` warnings to stderr even when it passes, so a run that
returns nothing at all is itself the signal that the terminal is lying.

## Proven

- The 3DGS lane compiles and its 237 crate tests pass on post-#130 main without
  reviving the retired contract target.
- `nerfstudio.splatfacto` cannot train on Apple Silicon: `splatfacto.py`
  hardcodes `.cuda()` (L208, L535) and `gsplat` builds CUDA extensions.
- Brush v0.3.0 CLI contract, read from upstream source: positional dataset path
  auto-disables the viewer; `--export-path` is a directory; `--export-name` is a
  filename joined onto it; `{iter}` is substituted with the zero-padded step; a
  final-step export is guaranteed (`iter % export_every == 0 || is_last_step`).
- Two silent-degradation modes are encoded as readiness preconditions and known
  limits: Brush trains from random initialization when the seed cloud is
  unreadable (info-level log only), and treats export failure as a warning. Exit
  status alone is therefore not evidence.
- Reacquisition is **expressible and computable** from an observer's own
  matrices, with no scene packet, checkpoint, or learned model.

## Not proven

State these explicitly; earlier summaries overstated them.

- **No trainer has ever been run.** The OpenSplat and Brush slices are launch
  *preparation* contracts. There is zero evidence that 3DGS works as a spatial
  memory backend.
- **Reprojection error is unmeasured.** The six reacquisition tests assert
  self-consistency against synthetic matrices, not agreement with a second
  viewpoint's own truth.
- **Occlusion is not handled** for reacquisition. See below.
- `checkpoint_native` is not learned inference: it records a `.ckpt` path as a
  witness and delegates projection to `projection_reference`, whose answer comes
  from recorded ground-truth geometry.
- **`populateNearbyBlocks` is compile-verified only.** `./gradlew compileJava`
  succeeds, but the mod has never been loaded into a running client since the
  change, so nothing here measures the actual per-tick cost of the surface scan
  (a radius-8 cube is 4913 `getBlockState` calls plus up to six neighbour
  lookups each) or the real per-line JSONL growth. Compilation is not behavior.
  The budget and radius constants are reasoned, not tuned against a measurement.

## Layered blockers

| Layer | Fact | Nature |
| --- | --- | --- |
| Contract | `TrainingResultSpatialQueryInputs` has no viewpoint parameter; `select_reference_frame` searches for a past frame that *saw* the target | Structural — the old path cannot express reacquisition |
| Capture | ~~`TelemetryRecorder` has no `populateNearbyBlocks`~~ — closed by `ef43bd92`. Existing captures are unaffected; only new recordings carry the field | Was a capability gap; now a re-capture requirement |
| Data | 34k frames hold exactly 1 camera pose, 1 unique hit block, 0 screenshots | Process only — the capture chain is already built, so this needs an operating session, not code. See the protocol section |
| Occlusion | A single centre-screen ray cannot witness occlusion for off-centre points; telemetry carries no depth | Signal coverage |

The layering matters: fixing capture and data still leaves reacquisition
unscorable until a viewpoint-conditioned contract exists, which is what the
`reacquisition` module now provides for the new path.

## Occlusion finding

`verify::evaluate_mismatch_refusal` already yields
`MismatchRefusalReason::TargetOccluded`: on a `Visible` projection it reads
`raycast_hit` and refuses when the first hit is a different block, and returns
`TelemetryUnreliable` when no hit witness exists.

**That rule does not transfer to reacquisition.** It is correct for aim
confirmation, where the crosshair is expected to be on the target, so a
mismatched hit implies something blocks it. Reacquisition asks where an
off-centre target sits, so `raycast_hit != target` is the normal case; reusing
the rule would report `TargetOccluded` for nearly every successful reacquisition.

Deciding occlusion for an arbitrary screen point needs a depth buffer, a ray
aimed at that specific target, or per-block visibility. The current telemetry
emits none of these. This is the first place in the lane where a depth map or a
learned/3DGS representation would add information that ground truth does not
already provide more cheaply.

## `nearby_blocks` sampling semantics (slice 2)

The wire shape and three Rust consumers already existed; only the producer was
missing. What the field now means:

- **Surface-exposed blocks only** — non-air with at least one air face
  neighbour. Interior blocks are excluded because they never render, so a 3DGS
  seed cloud initialized on them wastes primitives, and the aim-target consumers
  can only address faces a player could click. This also removes roughly an
  order of magnitude of volume per line.
- **Radius-bounded, not view-bounded.** Blocks behind the player are recorded.
  This is deliberately *not* a visibility claim. Frustum culling is deferred
  because the view/projection matrices exist only in the render phase, not the
  tick phase where sampling happens, and because "in frustum" is containment
  rather than visibility — the same distinction the occlusion finding above
  turns on. Marked `TODO(nearby-blocks-frustum-culling)`.
- **Nearest-first, budgeted per frame.** One JSONL line is written per rendered
  frame at client tick rate, so an unbounded surface set would add hundreds of
  entries per line. The cap is safe for the seed-cloud consumer because that
  cloud unions positions across frames and de-duplicates: per-frame sparsity
  still aggregates into coverage as the player moves. The two lookup consumers
  (`verify::target_block_id`, `select_reference_frame`) query one position at a
  time and need presence, not completeness.
- **Air-only exposure test.** A block whose sole opening is water or glass reads
  as interior and is dropped. The failure mode is under-reporting rather than
  recording buried geometry.

The regression test added with this slice pins the mod writer's exact byte shape
against the Rust reader. That branch of `appendNearbyBlocks` — element
separators plus the nested `block_pos` object — had never been exercised by real
data, and the existing ingest fixtures could not catch a mismatch because they
build a frame through serde and round-trip Rust's own output.

## Capture protocol for slice 3 (no new code required)

NOTICE (2026-09-12): the root `auv-minecraft` CLI was retired after this
handoff. The bash sketch below is a historical operating note, not a current
command surface. Do not recreate that binary. The `auv-cli` integration paths
in the table were retired with it; `projection_workflow.rs` now lives in
`supported/games/auv-game-minecraft/src/cli/` behind the `tracing` feature.
The library path that replaced the retired bridge for M1 is
`verify_m1_black_box_from_telemetry_tail` in `auv-game-minecraft`
(`read_latest_spatial_frame_from_tail` → `bind_capture_to_frame` →
`split_bound_frame_for_m1` → `prepare_m1_black_box_request` →
`inspect_m1_black_box_response` → `score_accepted_m1_black_box_response`).
Lower-level entry points remain public: `prepare_m1_black_box_from_telemetry_tail`
for request-only preparation, and the inspect/score helpers for fixture replay.
The live producer refuses `menu` and `loading_or_overlay` frames so a pause or
chunk overlay cannot be bound as an in-world RGB observation. Screenshot bytes on
Windows come from `auv invoke window.capture` (PrintWindow via `auv-driver-windows`,
canonical `auv://runs/...` artifact URI); pass that URI and the invoke capture
clock into `verify_m1_black_box_from_telemetry_tail` together with externally
produced model response JSON. When the capture clock is omitted, the newest in-game
sidecar timestamp is used. Live evidence on 2026-09-12: `window.list` resolved
`Minecraft* 1.21.1 - 单人游戏`, `window.capture --title Minecraft` returned
`backend=printwindow.windows` with no fallback and a non-black in-game PNG (not
WGC). Invoke JSON now exposes `result.capture.capture_monotonic_timestamp_ms`
(stamped immediately before PrintWindow / display capture). The scorer verification loop, calibration-curve aggregator
(`aggregate_m1_black_box_calibration_reports` → `M1BlackBoxCalibrationReport`),
and hermetic fixtures landed in 2026-09-12. Invoke JSON exposes
`result.capture.capture_monotonic_timestamp_ms`. A short-lived `auv invoke`
process originally stamped `0` via process-local `Instant`; Windows now uses
`GetTickCount64`.

Live M1 honesty-calibration evidence (2026-09-12, `.tmp/m1-baseline/`): five
`printwindow.windows` captures bound to `in_game` telemetry tails, scored against
external Claude CLI `SpatialHypothesisPatch` JSON. After the prompt named the
required unknown tokens, calibration reported `usable_count=5`, `usable_rate=1.0`,
`leak_count=0`, parallax follow-up on all five, `meets_m1_baseline_sample_gate=true`.
This **is** the M1 black-box honesty/calibration baseline. It is **not** pixel or
block hit-rate, **not** an in-crate VLM transport, and **not** M2.
Those five captures still used sidecar frame timestamps because the invoke stamp
was `0` at collection time.

### M2 multi-view capture gate (2026-09-12)

Library path in `auv-game-minecraft` (`m2_multi_view.rs`):

```text
M2ViewCaptureInput (frame or telemetry tail + screenshot URI + optional capture clock + role)
  -> bind_capture_to_frame
  -> split_bound_frame_for_m1
  -> prepare_m1_black_box_request (per view)
  -> pairwise withheld pose deltas
  -> meets_m2_capture_gate
  -> write_m2_session_report
```

Public types: `M2ViewRole` (`Anchor` / `Translate` / `Revisit`), `M2ViewSample`,
`M2RelativeMotion`, `M2Session`, `M2MultiViewSessionReport`. Gate threshold:
`M2_SIGNIFICANT_TRANSLATION_METERS = 0.5` (half-block eye translation). Rotation-only
sessions (yaw/pitch without ≥0.5 m translation) fail the gate. Serialized reports
exclude `nearby_blocks`, `view_matrix`, and absolute eye coordinates; motion is
relative deltas only. Withheld truth is available via `m2_session_withheld_truth`
for operator inspect, not for model requests.

Hermetic tests cover strafe pass, yaw-only fail, under-three fail, and JSON leakage
checks (`cargo test -p auv-game-minecraft --lib m2_`).

**Live M2 capture/binding evidence (2026-09-12, `.tmp/m2-session/`):** three
`printwindow.windows` captures bound to `in_game` telemetry tails via
`build_m2_session_from_captures` → `write_m2_session_report`. Roles:
`anchor` (v01) → `translate` (v02) → `revisit` (v03). Withheld eye translation:
anchor→translate **1.92 m**, anchor→revisit **2.64 m**, translate→revisit
**4.47 m**. `capture_monotonic_timestamp_ms` non-zero (`GetTickCount64`):
**9193984** / **9332234** / **9340390**; `capture_skew_ms`: **270** / **271** /
**264**. `session-report.json` reports `meets_m2_capture_gate=true` with no
`nearby_blocks`, `view_matrix`, or absolute eye coordinates in the serialized
report. VLM was intentionally skipped for this slice. NOTICE (clock domain): JVM
telemetry and AUV capture clocks are not calibrated — `capture_skew_ms` is
`frame_ts - capture_ts`, not a cross-domain wall-clock alignment proof. This
**is** the M2 multi-view capture/binding gate. It is **not** M3 memory/query
scoring, **not** M4 Brush/OpenSplat training, and **not** geometric accuracy
claims.

### Windows M1 PowerShell workflow (Phase 0 invoke producer)

NOTICE (clock domain): MC telemetry `monotonic_timestamp_ms` comes from the JVM
(`System.nanoTime()` scaled to ms). Invoke `capture_monotonic_timestamp_ms` comes
from the AUV capture-instant witness. On Windows that witness is `GetTickCount64`
(ms since boot) because a process-local `Instant` in short-lived `auv invoke`
stamped `0`. The two clocks do **not** share a base —
`bind_capture_to_frame` records skew as `frame_ts - capture_ts` and
`verify_m1_black_box_from_telemetry_tail` falls back to the sidecar frame timestamp
when the capture clock is omitted. Treat invoke clock as the capture-instant witness;
do not assume it is directly comparable to JVM nanoTime without calibration.

```powershell
$env:CARGO_TARGET_DIR = 'F:\auv\target'

# 1. List windows and confirm the Minecraft title
auv invoke window.list --json

# 2. Capture the in-game window (process name + title on Windows)
$captureJson = auv invoke window.capture --target javaw --title Minecraft --json | ConvertFrom-Json
$uri = $captureJson.artifacts[0].metadata.uri
$captureClockMs = $captureJson.result.capture.capture_monotonic_timestamp_ms

# 3. Run M1 verification (library) or hand uri + clock to an external VLM script
# verify_m1_black_box_from_telemetry_tail(telemetry_path, $uri, $captureClockMs, input_history)
# Example external script: .tmp/m1-vlm/invoke_m1_vlm.py (reads the same JSON fields)
```

`--target javaw` matches the Windows process executable name (`App::name`); on macOS
use `--target com.mojang.minecraftlauncher` (bundle id) instead.

Traced on 2026-07-27. Every link in the chain already exists; slice 3 is an
operating procedure, not an implementation.

The chain, with the code that already closes each link:

| Link | Where it already lives |
| --- | --- |
| Capture the client window through the driver | `capture_target_screenshot` in [`projection_workflow.rs`](../../../../../crates/auv-cli/src/integrations/minecraft/projection_workflow.rs) |
| Bind image to frame, record signed skew | `bind_capture_to_frame` → `mc_capture_skew_ms` |
| Publish PNG, then stamp the frame with its URI | `project_capture` sets `recorded_frame.screenshot_artifact_ref` before publishing the frame JSON |
| Pair frame↔screenshot inside a run | `resolve_spatial_frame_screenshot_bundle_ids` in [`mod.rs`](../../../../../crates/auv-cli/src/integrations/minecraft/mod.rs) parses `screenshot_artifact_ref` back into a bundle-local id |
| Merge many bundles into one packet | `export_3dgs_scene_packet` iterates `bundle_manifest_paths`; the CLI accepts `--bundle-manifest` repeatedly |

**One bridge invocation is one run.** `cli_frontend.rs` mints a fresh
`RunId::new()` per bridge call, so N poses produce N runs and N bundles. That is
fine — the scene packet merges them — but it means the procedure is
`bridge × N`, then `export-spatial-bundle × N`, then one
`export-3dgs-scene-packet`. Each bridge call prints `runId: <id>`; keep those.

Per pose (stand still, then run):

```bash
auv-minecraft bridge --sample "$HOME/Library/Application Support/minecraft/auv/telemetry.jsonl" --capture-target-app com.mojang.minecraft --target-block 513,72,726
```

Then per recorded run id, and finally once across all bundles:

```bash
auv-minecraft export-spatial-bundle <run-id> --output-dir out/bundles/<run-id>
```

Protocol constraints that decide whether the result is usable:

- **Move between poses.** The blocker this slice exists to clear is 34k frames
  holding one camera pose. Nothing in the pipeline enforces pose distinctness, so
  a session that samples from one spot reproduces the original blocker with more
  screenshots attached. Photogrammetry wants a baseline-to-depth ratio near 1:10,
  so for structures ~10 m away, move on the order of 1 m between samples.
- **Rotation is not a substitute for translation.** Pure yaw/pitch adds no
  parallax and cannot be triangulated. A panorama sweep from one position is the
  degenerate case, not a multi-pose capture.
- **Stand still while sampling.** `--capture-skew-ms` is a *declared* offset, not
  a measurement: `capture_timestamp` just shifts the frame timestamp by whatever
  the operator passes. Minecraft walking is ~4.3 m/s, so the live-click path's
  250 ms tolerance is ~1.1 m of travel — the same magnitude as the pose-spacing
  target above. Sampling while stationary is what makes the binding meaningful.
- **A refused projection still records the pair.** `project_capture` publishes
  the PNG and the stamped frame on both the `Bound` and `Refused` arms, so a pose
  where the target is occluded or out of frustum still contributes a usable
  frame↔screenshot pair. Refusals are expected in a multi-pose sweep and are not
  a protocol failure.

Measure on the first real session: slice 2 emits up to 128 `nearby_blocks` per
line and the mod writes one line per *rendered* frame regardless of whether that
frame was sampled. At 60 fps a several-minute session is tens of thousands of
lines, so record the actual per-line size and `telemetry.jsonl` growth. That
converts slice 2's unmeasured-growth limit into a measurement.

## Next slices, with unlock conditions

Ordered by dependency, not priority. Slices 1 and 2 landed on 2026-07-27; the
rest are not owner-approved.

1. ~~**Correct the occlusion marker**~~ — landed in `13216084`.
2. ~~**Mod: populate `nearby_blocks`**~~ — landed in `ef43bd92`. The
   `TODO(opensplat-seed-cloud-densification)` trigger is now reachable, but not
   yet true: it needs a *new* capture recorded with the updated mod, which makes
   it a consumer of slice 3 rather than something the mod change alone unlocks.
   The mod now also carries `TODO(nearby-blocks-frustum-culling)`, which points
   at slice 5.
3. ~~**Capture protocol: multi-pose plus screenshot binding (M2 gate).**~~ —
   **closed 2026-09-12.** Library gate in `m2_multi_view.rs` plus live Windows
   three-view evidence in `.tmp/m2-session/` (`meets_m2_capture_gate=true`). Still
   a precondition for slices 4 and 6. Scene-packet export across N runs and
   trainer launch remain unproven; M2 only closes capture/motion/binding honesty.
4. **Reacquisition scoring harness.** Anchor a target from viewpoint A, query
   from viewpoint B, score against B's own raycast and projection truth. Depends
   on 3. This is what turns the geometry backend from a candidate into a
   measured baseline.
5. **Occlusion signal decision.** Choose among depth buffer, target-directed
   ray, or per-block visibility, then define reacquisition's visibility
   semantics. Depends on 2 or a mod change.
6. **First real trainer run.** Depends on 3, since a single-pose capture cannot
   train.
7. **Replace `checkpoint_native` with real inference.** Depends on 6. Only
   meaningful once slice 4 provides a geometric baseline to compare against.

## Validation state

Commands run and results after slices 1 and 2:

- `cargo fmt --check` — passes.
- `cargo test -p auv-game-minecraft` — 238 passed, 0 failed (237 before slice 2's
  regression test).
- `cargo clippy -p auv-game-minecraft --all-targets` — 9 warnings, all
  pre-existing and none in a file this lane touched.
- `git diff --check` — clean.
- `./gradlew compileJava` in `devtools/auv-game-minecraft` — BUILD SUCCESSFUL.

**Mod build needs JDK 21.** The machine's default is JDK 25, and Gradle 8.14.3
rejects it with `Unsupported class file major version 69` while merely
*configuring* the build — the failure looks like a script bug, not a toolchain
mismatch, so it is worth naming here. The `java.toolchain` block in
`build.gradle` does not help, because the version that fails is the one running
Gradle itself:

```sh
JAVA_HOME="$(/usr/libexec/java_home -v 21)" ./gradlew compileJava
```

Three workspace test failures are unrelated to this lane and were each verified
against a pristine `origin/main` worktree:

- `auv-driver-linux` and `auv-driver-windows`
  `ocr::tests::rejects_buffer_with_mismatched_length` fail identically on
  pristine main; the platform stub returns `Unsupported` on macOS.
- `auv-tracing-conformance::concurrent_first_open_chooses_one_authority` passes
  3/3 in isolation on both pristine main and this branch, failing only under
  full parallel load. Resource-contention flake.

## Non-goals held throughout

- No CLI frontend wiring, and no `*_PURPOSE` artifact decision under the
  post-#130 recording contract.
- No `run_read.rs` 3DGS sections, no `inspect/render.rs`.
- No `artifact_roles.rs` restore.
- No integration into downstream consumers. Porting the query shape elsewhere
  was raised prematurely and withdrawn: there is no trained backend and no
  measured baseline to port.
