# 2026-06-27 AUV core spatial result consumption admission table

Date: 2026-06-27

Status: design-only admission verdict. This note classifies current
Minecraft MC-10 through MC-17 modules and symbols. It does **not** approve code
extraction by itself.

## Why this note exists

The pattern note in
`docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
froze the stage model:

```text
Producer Artifact
→ Semantic Gate
→ Spatial Query
→ Action Readiness View
→ Witness Artifact
→ Quality Measurement
```

That was still too abstract to guide code movement. This D2 note is the
admission verdict over the **actual** MC-10 through MC-17 implementation
surface.

The bar here is intentionally strict:

- if a symbol is still tied to Minecraft file layout, frame math, target
  semantics, or CLI shape, it stays local
- if a symbol is just repeated glue, it may become a helper later, but not a
  core contract
- if a symbol demonstrates a reusable contract, it is marked as a **candidate**
  only; the current Minecraft type still stays where it is

## Verdict meanings

| Verdict | Meaning |
| --- | --- |
| `keep app-specific` | Keep the symbol in Minecraft or CLI/read-side code. No extraction pressure yet. |
| `extract helper only` | A later shared helper may make sense, but helper extraction must not create a new core contract. |
| `candidate core contract` | The abstract rule looks reusable across verticals, but the current Minecraft symbol is still only a donor, not the final shared type. |
| `explicitly deferred` | Tempting abstraction, but it would widen scope or freeze the wrong boundary now. Leave it out on purpose. |

## Decision summary

### Keep app-specific

| Module | Concrete symbols | Verdict | Why |
| --- | --- | --- | --- |
| `crates/auv-game-minecraft/src/training_result_semantic.rs` | `validate_3dgs_training_result`, `TrainingResultSemanticManifest`, `TrainingResultSemanticInspectReport`, `TrainingResultSemanticReason`, `collect_checkpoint_files` | `keep app-specific` | Hard-wired to `config.yml`, `nerfstudio_models/`, top-level YAML `trainer`, and `*.ckpt`. This is semantic-gate evidence, but the current file contract is Minecraft/Nerfstudio-specific. |
| `crates/auv-game-minecraft/src/training_result_spatial_query.rs` | `query_3dgs_training_result`, `TrainingResultSpatialQueryManifest`, `TrainingResultSpatialQueryInspectReport`, `TrainingResultSpatialQueryRequest`, `TrainingResultSpatialQueryAnswer`, `TrainingResultSpatialQueryKind`, `TrainingResultSpatialQueryReason` | `keep app-specific` | The contract is built around block coordinates, block faces, Minecraft target semantics, scene-packet lineage, and projector math. |
| `crates/auv-game-minecraft/src/training_result_spatial_query.rs` | `run_projection_reference_backend`, `select_reference_frame`, `load_scene_packet_frame` | `keep app-specific` | Reference answering depends on `ScenePacketManifest`, `MinecraftProjector`, and MC frame payload layout. |
| `crates/auv-game-minecraft/src/training_result_spatial_query_provider.rs` | `run_checkpoint_native_provider_backend` | `keep app-specific` | This is a Minecraft/Nerfstudio adapter over normalized result directories and checkpoint naming, not a generic provider layer yet. |
| `crates/auv-game-minecraft/src/training_result_holdout_preview.rs` | `inspect_3dgs_training_result_holdout`, `TrainingResultHoldoutPreviewManifest`, `TrainingResultHoldoutPreviewInspectReport`, `HoldoutFrameWitness`, `HoldoutPreviewRequest`, `HoldoutPreviewAnswer`, `HoldoutPreviewReason`, `HoldoutFrameSelection` | `keep app-specific` | The witness artifact is real, but the current witness shape is still scene-packet frame JSON + screenshot + checkpoint path. |
| `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs` | `measure_3dgs_holdout_render_quality`, `TrainingResultHoldoutRenderQualityManifest`, `TrainingResultHoldoutRenderQualityInspectReport`, `HoldoutRenderQualityRequest`, `HoldoutRenderQualityAnswer`, `HoldoutRenderQualityReason`, `HoldoutRenderQualityMetrics` | `keep app-specific` | The quality seam still depends on MC-16 holdout witness fields, frame JSON, screenshot paths, and external command wiring. |
| `src/run_read.rs` | `extract_minecraft_training_result_semantic_*`, `extract_minecraft_training_result_spatial_query_*`, `extract_minecraft_training_result_holdout_preview_*`, `extract_minecraft_holdout_render_quality_*`, matching `list_*` entrypoints, and all `Minecraft*Summary` structs for these stages | `keep app-specific` | These readers encode current artifact roles and current JSON schemas. They are consumers of Minecraft truth, not a shared read API yet. |
| `src/inspect.rs` | `render_run_text` plus MC-10 / MC-12 / MC-14 / MC-16 / MC-17 section rendering | `keep app-specific` | This is current CLI inspection presentation, not a core inspection schema. |
| `src/inspect_server_viewer.html` | MC-10 / MC-12 / MC-14 / MC-16 cards and field rendering | `keep app-specific` | Viewer cards mirror one vertical’s artifact roles and naming. |
| `src/minecraft.rs`, `src/cli.rs`, `src/main.rs` | `run_minecraft_3dgs_training_result_semantic_validation`, `run_minecraft_3dgs_training_result_spatial_query`, `run_minecraft_3dgs_training_result_holdout_preview`, `run_minecraft_measure_3dgs_holdout_render_quality`, `parse_minecraft_validate_3dgs_training_result`, `parse_minecraft_query_3dgs_training_result`, terminal summary printing | `keep app-specific` | Runtime wrappers, CLI parsing, and terminal output belong to app/frontend ownership, not core extraction. |

### Extract helper only

| Module | Concrete symbols | Verdict | Why |
| --- | --- | --- | --- |
| `src/run_read.rs` | `read_artifact_json`, `open_artifact_file`, `is_json_mime` | `extract helper only` | These are plain artifact-read helpers. If another vertical repeats the same read pattern, extract a helper, not a new domain contract. |
| `src/inspect.rs` | `unique_matching_report` | `extract helper only` | This is a generic “pair only when exactly one business-key match exists” helper. It is useful infrastructure, but it does not define a domain contract by itself. |
| `src/inspect.rs` | `spatial_query_manifest_matches_report`, `holdout_preview_manifest_matches_report`, `holdout_render_quality_manifest_matches_report` | `extract helper only` | The reusable idea is business-lineage pairing over stable keys. The current functions are still MC field-by-field implementations. |
| `crates/auv-game-minecraft/src/training_result_spatial_query.rs` | `select_query_outcome`, `pick_blocked_or_failed_answer`, `compare_answers`, `answers_match` | `extract helper only` | The dual-backend compare policy may repeat, but the current implementation is still tied to MC-12 answer fields and block-projection semantics. |
| `crates/auv-game-minecraft/src/training_result_holdout_render_quality.rs` | `compute_rgb8_metrics` | `extract helper only` | RGB8 metric calculation is a measurement utility. If reused elsewhere, extract it as a narrow image-metric helper, not a core “quality framework”. |

### Candidate core contract

> Important: these are **donor symbols**, not approved shared types. The
> contract may graduate later; the current Minecraft enums and structs should
> still remain local for now.

| Current donor symbols | Candidate contract | Why this is a contract candidate | What must happen before extraction |
| --- | --- | --- | --- |
| `TrainingResultSemanticStatus`, `HoldoutPreviewStatus`, `HoldoutRenderQualityStatus` | `ready / blocked / failed` stage status triad | MC-10, MC-16, and MC-17 independently converged on the same honest stage-state split. | A second non-Minecraft vertical must need the same triad with the same blocked-vs-failed semantics. |
| `TrainingResultSpatialQueryStatus` | `answered / blocked / failed` query status | MC-12 and MC-15 show a distinct “query answered” state that should not collapse into generic readiness. | Another vertical needs target-conditioned answers, not just readiness gates. |
| `TrainingResultSpatialQueryComparisonVerdict` | provider/reference comparison verdict contract | `match / divergent / provider_only / reference_only / not_comparable` is a real cross-backend evidence shape. | Another vertical must actually run dual-backend compare and need the same verdict labels. |
| `TrainingResultSpatialQueryActionEligibility`, `TrainingResultSpatialQueryActionReadiness`, `derive_action_readiness`, `derive_minecraft_training_result_spatial_query_action_readiness` | action-readiness view contract | MC-14 proved a useful split between persisted query truth and derived action-facing consumability. | Another vertical must need “consume answer for action without dispatch” as a derived read model. |
| `HoldoutRenderQualityVerdict` | quality measurement evidence verdict | `measured_only / metric_partial / blocked / failed` is a defensible evidence-only measurement split. | Another vertical must measure quality against an authoritative witness without pretending to issue a threshold verdict. |
| `HoldoutRenderQualityBackend`, `TrainingResultSpatialQueryBackend` | stable persisted backend-label discipline | MC-12 and MC-17 both show the value of persisting backend labels instead of raw command text or transient runtime details. | A second vertical must need the same persistence rule around backend provenance. |

### Explicitly deferred

| Tempting move | Concrete symbols that make it tempting | Verdict | Why it is deferred now | Re-open trigger |
| --- | --- | --- | --- | --- |
| Generic `SpatialQueryProvider` trait | `run_command_provider_backend`, `run_checkpoint_native_provider_backend`, `run_projection_reference_backend` | `explicitly deferred` | One vertical is not enough. Extracting a provider trait now would freeze Minecraft’s backend split as fake core. | A second vertical with real dual-provider pressure, or explicit owner approval. |
| Generic render-provider abstraction | `run_external_holdout_render`, `run_external_holdout_render_quality`, `HoldoutPreviewRequest`, `HoldoutRenderQualityRequest` | `explicitly deferred` | MC-16 and MC-17 still use different command seams for different jobs. Unifying them now would blur witness selection and measurement. | A second witness/quality vertical that truly shares the same provider contract. |
| Runtime action wiring from derived readiness | `derive_action_readiness`, `derive_minecraft_training_result_spatial_query_action_readiness` | `explicitly deferred` | MC-14 intentionally stopped at read-side consumption. Dispatch belongs to a later action slice, not D2 core cleanup. | Owner-approved action slice that names the execution boundary. |
| Generic projected-point to action-point contract | `input_target::projected_window_point` | `explicitly deferred` | The current helper still carries an unresolved viewport-vs-window contract note. Promoting it now would lock in shaky geometry semantics. | Live telemetry or a second vertical confirms the coordinate contract. |
| Viewer unification | MC-10 / MC-12 / MC-16 / MC-17 sections in `src/inspect.rs` and cards in `src/inspect_server_viewer.html` | `explicitly deferred` | Presentation reuse is not the same thing as core contract reuse. Unifying the viewer first would front-run the domain decision. | At least two verticals need the same read-side cards over a stable shared contract. |
| Threshold-based quality verdicts | `HoldoutRenderQualityMetrics`, `HoldoutRenderQualityVerdict` | `explicitly deferred` | MC-17 is evidence-only by design. Adding thresholds now would collapse evidence into policy prematurely. | Owner-approved quality-policy slice with explicit thresholds and falsifiers. |

## Concrete conclusions

1. **No MC-10 through MC-17 module is ready for direct move into core as-is.**
   The current code is still mostly donor material plus app/read-side glue.
2. **The strongest real core candidates are status/verdict/readiness contracts,**
   not manifests, not provider adapters, and not viewer code.
3. **If D3 happens next, it should update terminology only.**
   It should not extract Rust code yet.
4. **If code extraction happens later, start with helper or enum-level moves,**
   not with a giant trait or a new generic runtime module.

## Direct sources

This admission verdict was grounded in:

- `docs/ai/references/2026-06-27-auv-core-spatial-result-consumption-pattern.md`
- `docs/ai/references/2026-06-27-minecraft-mc10-result-semantic-validation-design.md`
- `docs/ai/references/2026-06-27-minecraft-mc12-spatial-query-contract-design.md`
- `docs/ai/references/2026-06-27-minecraft-mc14-spatial-query-action-facing-consumer-design.md`
- `docs/ai/references/2026-06-27-minecraft-mc15-checkpoint-native-query-provider-seam-design.md`
- `docs/ai/references/2026-06-27-minecraft-mc16-holdout-preview-render-inspect-design.md`
- `docs/ai/references/2026-06-27-minecraft-mc17-holdout-render-quality-design.md`
- the concrete Rust modules named in the tables above
