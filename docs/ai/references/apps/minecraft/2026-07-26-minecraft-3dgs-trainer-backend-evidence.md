# Minecraft 3DGS trainer backend evidence (2026-07-26)

Durable record of which 3DGS trainer backends can actually run on the owner's
target hardware (Apple Silicon M4, 16 GB), and what the launch-preparation
contract must therefore encode. Supersedes the assumption that
`nerfstudio.splatfacto` is a reachable default.

Evidence level: **upstream source read** (GitHub raw / API, fetched
2026-07-26). No trainer was installed and no training run was executed, so
nothing here is a runtime-performance claim.

## Finding: the pinned nerfstudio backend is unreachable on Apple Silicon

`nerfstudio.splatfacto` cannot train on an M4 regardless of PyTorch MPS
support:

- `nerfstudio/models/splatfacto.py` hardcodes `.cuda()` at L208
  (`torch.zeros(...).float().cuda()`) and L535
  (`camera.get_intrinsics_matrices().cuda()`). These are unconditional, not
  device-dispatched.
- Its rasterizer `nerfstudio-project/gsplat` builds CUDA extensions
  (`setup.py` L203 `from torch.utils.cpp_extension import CUDAExtension`,
  L226, L249) and names NVIDIA as the hardware requirement in its README.

`PYTORCH_ENABLE_MPS_FALLBACK` does not help: the failure is a hard CUDA
dependency in the extension build, not an unimplemented MPS operator.

Consequence for AUV: a launch plan that pins one backend was claiming
readiness for a trainer that can never produce a `.ply` on the target machine.
`TrainingBackend` exists to make that explicit rather than implied.

## Candidate backends

| Backend | Apple Silicon | Distribution | Fit for AUV's `sh -lc` seam |
| --- | --- | --- | --- |
| `nerfstudio.splatfacto` | No (CUDA-hardcoded, above) | pip + CUDA build | Unreachable on target |
| OpenSplat | Yes (MPS) | Source build: libtorch + OpenCV + Xcode | Works, heavy toolchain |
| Brush | Yes (wgpu/Metal) | Prebuilt `aarch64-apple-darwin` binary | Best fit |
| gsplat-mlx ports | Research templates | No scene loader / pinned conda env | Not a trainer binary |

### Brush is the strongest fit for the existing seam

`ArthurBrussee/brush`, Rust + wgpu/Burn. Release `v0.3.0` ships
`brush-app-aarch64-apple-darwin.tar.xz`, so the M4 needs no build toolchain at
all — no libtorch, OpenCV, conda, or Xcode chain to reproduce.

Contract details that matter, read from source:

- Binary is `brush` (`apps/brush-app/Cargo.toml` `[[bin]]`). It runs headless
  automatically: `with_viewer` is declared
  `default_value_if("source", ArgPredicate::IsPresent, "false")`, so passing a
  positional source path disables the viewer. No window server required.
- The positional source accepts a plain directory (`DataSource::Path` via
  `FromStr` for any non-http string; `brush-vfs` walks directories), so AUV can
  hand it `compat/nerfstudio/` directly with no packing step.
- `crates/brush-dataset/src/formats/nerfstudio.rs` parses the same field set
  `training_package.rs` already emits: `camera_model`, `w`, `h`, `fl_x`,
  `fl_y`, `cx`, `cy`, `k1`, `k2`, `p1`, `p2`, `frames[].file_path`,
  `frames[].transform_matrix`. Zero format-conversion work.
- **Axis convention confirmed compatible.** The reader calls
  `opengl_c2w_to_pose(transform)` on each `transform_matrix`, i.e. it expects
  OpenGL camera-to-world — which is what AUV's GL-derived export already
  writes. No axis flip is needed, and adding one would corrupt the scene.

### Brush accepts AUV's seed cloud shape as-is

`crates/brush-serde/src/import.rs` treats Gaussian attributes as optional and
fills defaults for whatever is absent: `rotations`, `log_scales`,
`raw_opacities`, and `sh_coeffs` are each gated on
`vertex.has_property("rot_0" | "scale_0" | "opacity")`. It also *prefers*
vertex color when present, converting `red`/`green`/`blue` through
`rgb_to_sh`. A bare `XYZ + uchar RGB` cloud — exactly what
`write_seed_point_cloud` emits — is therefore a valid initialization input.

## Known limits and boundaries

- **Brush seeding fails silently.** `nerfstudio.rs` L369-380 wraps the seed
  read in `if let Ok(ply_data)`; the `Err` arm leaves `init_splat = None` and
  training proceeds from random init. A missing or misnamed `ply_file_path`
  degrades quietly instead of erroring. Any AUV-side readiness check must
  assert the seed file exists before launch and must not treat exit code 0 as
  proof the seed was used. This is the opposite failure mode from OpenSplat,
  whose nerfstudio reader aborts on an empty `ply_file_path`.
- OpenSplat has two open MPS defects (upstream #198 NaNs / command-buffer
  aborts, #77 MPS memory leak) that are exactly the failure modes a 16 GB
  machine hits first.
- `v0.3.0` is dated 2025-09-14; the Brush CHANGELOG `Unreleased` section lists
  later training-performance, densification, and random-frustum-init work not
  in that tarball. Building `main` instead means depending on `burn`/`burn-wgpu`
  git-branch pins rather than crates.io versions.
- Not verified: whether the shipped macOS binary is codesigned/notarized. If
  it is not, first-run Gatekeeper quarantine could block an unattended
  `sh -lc` launch.
- Not verified: actual wall-clock or peak memory for a training run on
  M4/16 GB. Running a trainer was explicitly out of scope.

## Deferred, with owner decisions outstanding

- Adding a `TrainingBackend::Brush` variant. `TrainingBackend` currently covers
  `NerfstudioSplatfacto` and `OpenSplat` only; the evidence above argues Brush
  should be the target on this hardware, but the variant is not implemented and
  needs the owner to name that slice.
- Re-exposing 3DGS training on the CLI. #130 deleted the command variant, the
  parser and dispatch arms, `artifact_roles.rs`, and the recording API the old
  wiring used. Restoring it requires a new `*_PURPOSE` artifact decision under
  the post-#130 `Context`/`publish_*` contract, so it is a design slice rather
  than a port.
- Densifying the seed cloud from `nearby_blocks`, tracked in code as
  `TODO(opensplat-seed-cloud-densification)`. One point per raycast-hit block
  is a very sparse initialization.
