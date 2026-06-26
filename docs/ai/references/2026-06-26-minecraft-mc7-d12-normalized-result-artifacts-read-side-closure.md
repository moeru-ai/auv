# MC-7 D12 normalized result artifacts read-side closure

## Summary

MC-7 D12 closes the read-side consumer gap for D11 normalized training result
artifacts. D11 already writes the normalized artifact manifest and inspect
report; D12 makes those artifacts stable in run reading, inspect text, and the
inspect viewer.

This is an evidence-consumption closure, not a model-quality closure. D12 does
not evaluate trained splats, inspect checkpoint internals, preview a model, or
claim that a real remote training result is useful for downstream rendering.

## Consumer contract

D12 consumes the existing D11 artifact roles:

- `minecraft-3dgs-training-result-artifact-manifest`
- `minecraft-3dgs-training-result-artifact-inspect`

The manifest consumer exposes normalized artifact rows for:

- `config`
- `models_directory`
- optional `status_snapshot`

Each row keeps the D11 fields: `kind`, `relative_path`, `absolute_path`,
`readable`, and `byte_size`.

The inspect consumer keeps the D11 fetch status fields visible:
`fetch_status`, `fetch_reason`, `source_result_dir_exists`,
`required_artifacts_present`, `normalized_artifact_count`, `warnings`, and
`known_limits`.

## What changed

- `run_read` tests now cover D11 normalized artifact rows and blocked inspect
  summaries as stable read-side data.
- `inspect` text now renders normalized artifact rows instead of only the total
  count.
- The inspect viewer recognizes both D11 roles and renders lightweight summary
  cards before the raw JSON.
- The raw JSON artifact text and download path remain available.

## MC-7 closure wording

After D12, MC-7 can be described as closed for trainer-side lineage and evidence
consumption:

```text
training package -> launch prep -> job envelope -> result collection
  -> normalized result artifacts -> read-side / inspect / viewer consumer
```

Do not describe this as trained model quality validation. A future quality or
splat-consumption slice must use a separate acceptance gate.

## Boundaries

D12 intentionally does not:

- change D11 producer schema or artifact role names;
- add a new CLI command;
- refetch or normalize result files;
- parse `nerfstudio_models/` internals;
- inspect checkpoints;
- add a 3D viewer or rendering preview;
- depend on historical MC-6 accepted runs.
