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

Restore drift was mechanical: nine `auv_tracing_driver::now_millis()` call sites
became the crate's existing `crate::now_millis()`. `artifact_roles.rs` was
**deliberately not restored** — `TELEMETRY_SAMPLE_ARTIFACT_ROLE` no longer
exists, `dataset.rs` now owns the bundle role constants, and the restored set
references zero constants from it.

Deliberately excluded reconnection surface: `run_read.rs` 3DGS sections,
`inspect/render.rs`, all CLI wiring. `tests/feature_boundary.rs` therefore still
passes — the retired contract target was not revived.
## Uncommitted working-tree state

1. `cargo fmt` repair of `247e9185`, which shipped unformatted code in
   `reacquisition.rs` and `lib.rs`. The earlier "fmt passed" claim for that
   commit was wrong: it rested on a terminal that returned exit 0 with empty
   output. Violations were confined to those two files.
2. Five deferral markers added to `reacquisition.rs` (occlusion, unmeasured
   error, confidence placeholder, status-mapping notice, module evidence
   boundary).
3. **Outstanding**: the `TODO(reacquisition-occlusion)` rationale is still wrong
   and needs the correction described below.

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
## Layered blockers

| Layer | Fact | Nature |
| --- | --- | --- |
| Contract | `TrainingResultSpatialQueryInputs` has no viewpoint parameter; `select_reference_frame` searches for a past frame that *saw* the target | Structural — the old path cannot express reacquisition |
| Capture | `TelemetryRecorder` has no `populateNearbyBlocks`; the field is serialized but never filed | Capability gap — queryable targets reduce to crosshair-hit blocks |
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
## Next slices, with unlock conditions

Ordered by dependency, not priority. None is owner-approved yet.

1. **Correct the occlusion marker** (docs-only). Replace the inaccurate
   rationale with the `verify.rs` non-transferability finding above. Ready now.
2. **Mod: populate `nearby_blocks`** (`devtools/auv-game-minecraft`, Java).
   Sole precondition for the `TODO(opensplat-seed-cloud-densification)` gap; the
   TODO's stated trigger ("once telemetry populates nearby_blocks") cannot
   become true on its own.
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

Commands run and results at handoff:

- `cargo fmt --check` — passes after the working-tree repair.
- `cargo test -p auv-game-minecraft` — 237 passed, 0 failed.
- `cargo check --workspace --all-targets` — 0 errors.
- `git diff --check` — clean.

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
