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
| Data | 34k frames hold exactly 1 camera pose, 1 unique hit block, 0 screenshots | Process — no parallax, so no reacquisition scoring is possible |
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
3. **Capture protocol: multi-pose plus screenshot binding.** Requires the owner
   to operate the client. Precondition for slices 4 and 6.
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
