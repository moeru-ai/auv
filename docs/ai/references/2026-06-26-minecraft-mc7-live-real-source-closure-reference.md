# MC-7 fresh live real-source closure reference

Date: 2026-06-26

## Summary

This note records the fresh MC-7 real-source closure run. It does not rely on the
lost historical accepted-only `.auv` lineage. The closure starts from a new
Minecraft live sweep and proceeds through the accepted-only MC-7 scene packet,
training package, training launch prep, remote-job envelope, result collection,
and read-side inspect smoke.

This is not a trained-model quality gate. The final trainer-side state is
blocked because no remote job configuration is present and `ns-train` is not
installed locally. The useful closure here is that the evidence chain is fresh,
auditable, and consumable by inspect.

## Live Capture Shape

Target block: `511,73,728`

The source run shape was three profiles, each with two accepted in-game bridge
runs and one live menu refusal run:

| Profile | Telemetry session | Accepted early | Accepted late | Refusal menu |
| --- | --- | --- | --- | --- |
| rich | `8a704135-a7e0-43a2-803f-1d71d59b3173` | `run_1782439609675_71442_0` | `run_1782439657402_72630_0` | `run_1782439736167_74554_0` |
| flat_color | `f766d8ef-d7c5-492a-86fa-1a25286e2a94` | `run_1782439895856_78484_0` | `run_1782439964390_80133_0` | `run_1782440208995_85685_0` |
| repetitive | `71475a22-0600-492e-b5f4-fc5f71b76856` | `run_1782440576466_93339_0` | `run_1782440921236_1495_0` | `run_1782441165813_7245_0` |

Accepted-only MC-7 D2 input runs were the six accepted runs above. The refusal
runs remain side evidence for live refusal behavior and are intentionally not
mixed into the accepted-only scene packet.

## D2 Scene Packet

Command:

```sh
cargo run --quiet -- minecraft export-3dgs-scene-packet \
  --bundle-manifest .tmp/mc7-live/closure/bundles/rich-early/run.json \
  --bundle-manifest .tmp/mc7-live/closure/bundles/rich-late/run.json \
  --bundle-manifest .tmp/mc7-live/closure/bundles/flat-early/run.json \
  --bundle-manifest .tmp/mc7-live/closure/bundles/flat-late/run.json \
  --bundle-manifest .tmp/mc7-live/closure/bundles/repetitive-early/run.json \
  --bundle-manifest .tmp/mc7-live/closure/bundles/repetitive-late/run.json \
  --output-dir .tmp/mc7-live/closure/scene-packet
```

Recorded run: `run_1782441338549_12179_0`

Key inspect result from `.tmp/mc7-live/closure/scene-packet/inspect_report.json`:

- `frames = 6`
- `screenshots = 6`
- `missing_screenshots = 0`
- `camera_records = 6`
- `source_runs = 6`
- `resource_pack_profiles = 3`
- anomaly arrays are empty

Resource-pack coverage:

| Resource pack | Frames | Source runs | Timestamp range |
| --- | ---: | --- | --- |
| `file/auv-mc6-rich` | 2 | `run_1782439609675_71442_0`, `run_1782439657402_72630_0` | `46387822` → `46435507` |
| `file/auv-mc6-flat-color` | 2 | `run_1782439895856_78484_0`, `run_1782439964390_80133_0` | `46673978` → `46742527` |
| `file/auv-mc6-repetitive` | 2 | `run_1782440576466_93339_0`, `run_1782440921236_1495_0` | `47354539` → `47699346` |

## D3 Training Package

Command:

```sh
cargo run --quiet -- minecraft export-3dgs-training-package \
  --scene-packet-manifest .tmp/mc7-live/closure/scene-packet/run.json \
  --output-dir .tmp/mc7-live/closure/training-package
```

Recorded run: `run_1782441363311_12947_0`

Key inspect result from `.tmp/mc7-live/closure/training-package/inspect_report.json`:

- `frames = 6`
- `images = 6`
- `compatibility_view = nerfstudio`
- `compatibility_status = ready`
- `compatibility_exported_frames = 6`
- `compatibility_skipped_frames = 0`
- `transforms_path = compat/nerfstudio/transforms.json`
- known limit: legacy rotation-only `view_matrix` fallback was reused on frame indices `1,2,3,4,5,6`

## D5 Training Launch Prep

Command:

```sh
cargo run --quiet -- minecraft prepare-3dgs-training \
  --training-package-manifest .tmp/mc7-live/closure/training-package/run.json \
  --output-dir .tmp/mc7-live/closure/training-launch
```

Recorded run: `run_1782441383547_13528_0`

Key inspect result from
`.tmp/mc7-live/closure/training-launch/minecraft-3dgs-training-launch-inspect.json`:

- `compatibility_status = ready`
- `trainer_readiness = blocked`
- `readiness_blocker = trainer_command_unavailable`
- `probe_command = ns-train --help`
- `probe_succeeded = false`
- `exported_frame_count = 6`
- `skipped_frame_count = 0`
- `transforms_present = true`

## D6 Remote Training Job Envelope

Command:

```sh
cargo run --quiet -- minecraft launch-3dgs-training-job \
  --training-launch-plan .tmp/mc7-live/closure/training-launch/minecraft-3dgs-training-launch-plan.json \
  --output-dir .tmp/mc7-live/closure/training-job
```

Recorded run: `run_1782442440013_41462_0`

Key inspect result from
`.tmp/mc7-live/closure/training-job/minecraft-3dgs-training-job-inspect.json`:

- `status = blocked`
- `job_backend = remote`
- `trainer_backend = nerfstudio.splatfacto`
- `job_submission_endpoint = unconfigured`
- `readiness_blocker = missing_configuration`
- `job_id = null`
- `job_url = null`
- `exported_frame_count = 6`
- `skipped_frame_count = 0`
- `transforms_present = true`

During this live run, D6 exposed a schema-consumption bug: it attempted to parse
D3 `training-package/inspect_report.json` as a D5 `TrainingLaunchInspectReport`.
The fix is to parse it as `TrainingPackageInspectReport`, because the D5 launch
plan intentionally references the D3 package inspect report.

## D7 Training Result Collection

Command:

```sh
cargo run --quiet -- minecraft collect-3dgs-training-job-result \
  --training-job-manifest .tmp/mc7-live/closure/training-job/minecraft-3dgs-training-job.json \
  --output-dir .tmp/mc7-live/closure/training-result
```

Recorded run: `run_1782442607642_46446_0`

Key inspect result from
`.tmp/mc7-live/closure/training-result/minecraft-3dgs-training-result-inspect.json`:

- `status = blocked`
- `status_reason = launch_blocked`
- `source_job_status = blocked`
- `job_backend = remote`
- `trainer_backend = nerfstudio.splatfacto`
- `job_submission_endpoint = unconfigured`
- `job_id = ""`
- `result_dir_exists = false`
- `key_result_artifacts_present = false`
- `result_artifact_count = 2`

During this live run, D7 exposed a blocked-job consumption bug: D6 blocked jobs
may legitimately have no `job_id`, but D7 hard-failed before writing blocked
result evidence. The fix is narrow: submitted jobs without `job_id` still hard
fail, while blocked jobs without `job_id` write a blocked result with
`status_reason = launch_blocked`.

## D8 Inspect Smoke

Inspect smoke commands were run against the recorded D3/D5/D6/D7 runs:

```sh
cargo run --quiet -- inspect run_1782441363311_12947_0
cargo run --quiet -- inspect run_1782441383547_13528_0
cargo run --quiet -- inspect run_1782442440013_41462_0
cargo run --quiet -- inspect run_1782442607642_46446_0
```

The text inspect output showed all trainer-side read sections:

- `MC-7 Training Packages:`
- `MC-7 Training Launches:`
- `MC-7 Training Jobs:`
- `MC-7 Training Results:`

Paired inspect artifacts were rendered for the D3 package, D5 launch, D6 job,
and D7 result records via `paired_report_artifact=` lines.

## Current Closure State

- Fresh accepted-only real-source lineage exists and was exported through D2.
- D3 Nerfstudio compatibility view is `ready` with 6 exported frames.
- D5 training launch prep is blocked only because local `ns-train` is unavailable.
- D6 remote job envelope is blocked because remote job configuration is absent.
- D7 result collection records that blocked state instead of pretending a model exists.
- D8 read-side inspect can consume and display D3/D5/D6/D7 trainer-side lineage.

This closure does not claim trained splat quality, trainer success, or model
readiness. The next real trainer step requires configuring a remote job backend
or explicitly installing a local trainer path in a separate slice.
