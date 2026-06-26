# MC-8 closure and gate verdict

Date: 2026-06-26

## Summary

MC-8 closes the **remote command-adapter lane** for the Minecraft trainer-side
artifact chain. It does not close a provider-backed remote trainer lane, and it
does not claim model quality, renderer quality, checkpoint semantics, or splat
usefulness.

The closed MC-8 surface is:

```text
D1 remote submit adapter
-> D2 remote status/result adapter
-> D3 remote artifact fetch adapter
-> D4 adapter live success evidence
-> D12 read-side inspect consumption
```

The right verdict is therefore:

- **closed:** command-adapter live closure
- **not closed:** provider-backed remote training reality closure

## Input references

MC-8 D5 is a documentation and verdict slice over already-landed evidence. The
primary references are:

- `docs/ai/references/2026-06-26-minecraft-mc8-d1-d3-remote-adapter-closure.md`
- `docs/ai/references/2026-06-26-minecraft-mc8-d4-adapter-live-closure.md`
- `docs/ai/references/2026-06-18-auv-mc5-onward-execution-plan.md`

## What MC-8 closed

### D1 — remote submit adapter

MC-8 D1 closed the command surface for D6 remote submission:

- explicit endpoint/token/submit-command inputs exist;
- submit adapters consume JSON on stdin and return JSON on stdout;
- missing or malformed submission evidence is recorded honestly as blocked or
  failed instead of pretending the remote lane succeeded.

Reference:
`docs/ai/references/2026-06-26-minecraft-mc8-d1-d3-remote-adapter-closure.md:21`

### D2 — remote status/result adapter

MC-8 D2 closed the **command-adapter lane** for D7 remote status/result
collection. That lane is distinct from the later MC-9 D3 **real provider
status** lane documented in
`docs/ai/references/2026-06-27-minecraft-mc9-d3-real-provider-status-closure.md`.

MC-8 D2 closed the command surface for D7 remote status/result collection:

- explicit status-command input exists;
- explicit commands are exercised as the adapter seam;
- blocked / failed / succeeded states are recorded into the existing trainer-side
  manifest and inspect surfaces.

Reference:
`docs/ai/references/2026-06-26-minecraft-mc8-d1-d3-remote-adapter-closure.md:40`

### D3 — remote artifact fetch adapter

MC-8 D3 closed the command surface for D11 normalized result fetch:

- explicit artifact-fetch-command input exists;
- normalized result artifacts are materialized under `normalized-result/`;
- required trainer-side artifacts remain the same:
  - `config.yml`
  - `nerfstudio_models/`
  - optional `job_status.json`

Reference:
`docs/ai/references/2026-06-26-minecraft-mc8-d1-d3-remote-adapter-closure.md:58`

### D4 — adapter live gate

MC-8 D4 then ran the hardened adapter path through live command adapters and
recorded a non-blocked evidence chain:

- D6 submit run: `run_1782474146691_75094_0`
- D7 result run: `run_1782474150531_75179_0`
- D11 local fetch run: `run_1782474152595_75586_0`
- D11 command fetch run: `run_1782474155519_75839_0`

The resulting evidence shows:

- D6 submitted with real adapter-facing `job_id` / `job_url`;
- D7 succeeded and the explicit status command saw stdin `job_id`;
- D11 succeeded in both the locally-readable path and the command-materialized
  path;
- D12 inspect consumed the paired report rows and normalized artifact rows.

Reference:
`docs/ai/references/2026-06-26-minecraft-mc8-d4-adapter-live-closure.md:45`

## Evidence checklist

The minimum evidence that supports the MC-8 closure verdict is:

- D1-D3 contract/boundary note:
  `docs/ai/references/2026-06-26-minecraft-mc8-d1-d3-remote-adapter-closure.md`
- D4 live success note:
  `docs/ai/references/2026-06-26-minecraft-mc8-d4-adapter-live-closure.md`
- D4 recorded run ids:
  - `run_1782474146691_75094_0`
  - `run_1782474150531_75179_0`
  - `run_1782474152595_75586_0`
  - `run_1782474155519_75839_0`

Local `.tmp/mc8-d4-recorded/` inspect snapshots may still exist as operator-side
working copies, but they are not part of the committed minimum evidence for the
MC-8 closure verdict.

## Explicitly not closed by MC-8

MC-8 does **not** close any of the following:

- provider-backed remote trainer execution;
- cloud-specific auth/runtime correctness;
- local or remote `ns-train` quality;
- trained splat usefulness;
- renderer / viewer / preview quality;
- checkpoint internals or checkpoint semantic validation;
- new Minecraft capture collection.

Those belong to later slices and must not be backfilled into the MC-8 verdict.

## Gate verdict

### Closed

MC-8 is closed for the **command-adapter live gate**.

That means the repository now has a proven, auditable path for:

- submitting a remote trainer-side job through a JSON/stdin/stdout adapter;
- collecting objective remote status/result state through a command adapter;
- normalizing trainer-side result artifacts through either a local-copy path or
  a command-materialized fetch path;
- consuming the resulting lineage through `auv-cli inspect` without changing the
  persisted trainer-side artifact roles.

### Not closed

MC-8 is **not** closed for a real provider-backed remote training service.

Nothing in MC-8 proves that an external provider actually trained a model,
stored a valid checkpoint set, or produced a useful splat. The D4 live success
run proves the adapter seam and read-side closure only.

## Final wording

Use this wording for follow-up summaries and planning notes:

> MC-8 closes the remote command-adapter lane through D1-D4 and D12. It does not
> close provider-backed remote training reality, model quality, renderer quality,
> or checkpoint semantics.
