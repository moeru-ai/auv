# NetEaseMusic V2 Candidate Pass

Date: 2026-05-20

Status: candidate-aware evidence report

## Purpose

This report records one full NetEaseMusic phase-2 pass:

```text
app probe -> app analyze -> app distill -> app validate
```

The goal was not to finish a general NetEaseMusic skill. The goal was to check
whether the current V2 workflow can preserve enough truth to explain what did
and did not work.

## Pass Inputs

- target bundle id: `com.netease.163music`
- app name from LaunchServices: `NeteaseMusic`
- visible localized foreground app name: `网易云音乐`
- app version: `3.1.7`
- build version: `3283`
- pass directory:
  `.auv/app-passes/2026-05-20-netease-full-v2-candidate-pass`
- source commit during the pass: `f183527`

Commands:

```bash
cargo run --quiet -- app probe com.netease.163music \
  --output-dir .auv/app-passes/2026-05-20-netease-full-v2-candidate-pass/probe

cargo run --quiet -- app analyze \
  .auv/app-passes/2026-05-20-netease-full-v2-candidate-pass/probe

cargo run --quiet -- app distill \
  .auv/app-passes/2026-05-20-netease-full-v2-candidate-pass/probe/analysis.json \
  --output-dir .auv/app-passes/2026-05-20-netease-full-v2-candidate-pass/distill

cargo run --quiet -- app validate \
  .auv/app-passes/2026-05-20-netease-full-v2-candidate-pass/distill
```

Run ids:

- probe: `run_1779249198429_79077_0`
- analyze: `run_1779249223158_80014_0`
- distill: `run_1779249231300_80103_0`
- validate: `run_1779249239348_80205_0`

## 1. Probe Live Surface

The live surface is usable, but not fully coherent yet.

Good signals:

- app identity resolved through LaunchServices
- Screen Recording, Accessibility, and Automation to System Events were granted
- `debug.captureDisplay` captured `display_0`
- `debug.observeWindowTree` saw an AX root window and basic titlebar controls
- `debug.captureWindow` later captured the target window during validation

Weak or inconsistent signals:

- `debug.observeWindows` reported `windowCount=0` even though the frontmost app
  was `网易云音乐` with bundle id `com.netease.163music`
- the later `debug.clickWindowPoint` validation step resolved the same app by
  `bundle-id-exact` and selected `windowRef=3178`
- the AX tree contained only 5 nodes: root window plus titlebar buttons/groups
- the OCR probe summary said zero filtered matches because the sample threshold
  was `min_confidence=0.55`
- the OCR artifact still recorded two raw observations for `网易云音乐`, both at
  confidence `0.300`

Interpretation:

`observeWindows` is not yet selector-coherent for this app in the probe path.
The probe passes `com.netease.163music`, but the window script filters by owner
name text, so the localized owner name `网易云音乐` does not match the bundle id.
Validation later succeeds because `clickWindowPoint` uses an unfiltered
128-window snapshot and resolves by owner bundle id.

That means the app surface is not broken, but the probe window selector is still
too weak to be the trusted source of window truth.

## 2. Annotation Candidate Quality

`app analyze` produced 3 annotations:

- `window-primary-region`
- `ocr-anchor-0`
- `ocr-anchor-1`

The current candidate classification avoided the most dangerous overclaim:

- `ocr-anchor-0` was `area=ocr-visible-text`, `status=partial`
- `ocr-anchor-1` was `area=ocr-visible-text`, `status=partial`
- neither OCR anchor had `direct_taxonomy_ids`
- neither OCR anchor was treated as `result-selection`

This is correct for the observed surface. Both OCR anchors are title-level app
labels, not result rows:

- `• 网易云音乐`
- `⑥ 网易云音乐`

There was no semantic result evidence and no visible-row candidate. The current
pass therefore did not prove a NetEaseMusic list/result grounding path.

One truth-level issue remains: `grounding_assessment.ocr_sample_status` became
`candidate` because analysis counted raw OCR observations, while the live probe
summary said zero filtered matches under the active confidence threshold. V2
should carry both values explicitly:

- raw OCR observation count
- filtered OCR match count under the probe threshold

Without that split, low-confidence title labels can make the analysis sound
more grounded than the probe actually was.

## 3. Distill Annotation References

`app distill` produced 1 candidate:

- `macos.neteasemusic.window_action_candidate.v0`
- taxonomy: `window-action.window-point.pointer-click.capture-evidence`

The candidate referenced the right annotation:

- `suggested_annotation_ids = ["window-primary-region"]`
- `candidate_shape.direct_candidate_ids = ["window-primary-region"]`
- `candidate_shape.provided_inputs.relative_x = "0.500000"`
- `candidate_shape.provided_inputs.relative_y = "0.500000"`
- `candidate_shape.provided_inputs.window_bounds = "226,101,1060,752"`

This is the right behavior for the evidence available. Distill did not invent a
search-entry candidate, result-selection candidate, or playback skill.

## 4. Validate Annotation Usage

`app validate` recorded the annotation usage correctly:

- status: `validated`
- verification mode: `evidence-only`
- `used_annotation_ids = ["window-primary-region"]`
- `resolved_inputs.relative_x = "0.500000"`
- `resolved_inputs.relative_y = "0.500000"`
- `unresolved_inputs = []`

The validation step executed:

1. foreground NetEaseMusic
2. click the center of the resolved primary window
3. capture post-click window evidence

The click evidence shows:

- match strategy: `bundle-id-exact`
- resolved app name: `网易云音乐`
- window bounds: `226,101,1060,752`
- resolved logical point: `756,477`
- window relative point: `0.500,0.500`

The post-click screenshot shows that the click changed the UI into a playlist
view (`520必听 | 热门流行陪伴你和TA`). That proves the pointer action had a real
UI effect, but it does not prove any semantic objective. It is still only an
activation-level window action.

## Failure Classification

Initial primary finding before `bf873de`:

- `selector/window` is the first hard issue. The probe window observation path
  returned zero windows for a bundle-id target, while validation later resolved
  the same app/window through a different selector path.

This was the right first blocker at source commit `f183527`. It was addressed
by `bf873de`; see the live regression below.

Secondary findings:

- `candidate taxonomy / truth levels` needs tightening around OCR confidence.
  Raw low-confidence OCR observations should not make the grounding assessment
  sound equivalent to filtered matches.
- `list candidate insufficiency` remains true. The pass produced no
  result-list, row, song, playlist, or search-result candidate.
- `control/click` is mechanically working for window-relative activation, but
  the current target spec is just the window center. It is not a semantic
  affordance.
- `verification provider` is not the blocker for the current window-action
  slice because the slice is deliberately `evidence-only`.
- `semantic mismatch` is expected: clicking the window center activated a
  playlist card, not a requested domain action.

## `bf873de` Live Regression

After `fix(macos): resolve observe windows by app selector`, the selector/window
path was rerun against the live desktop.

Direct regression command:

```bash
cargo run --quiet -- invoke debug.observeWindows \
  --target com.netease.163music \
  --limit 20
```

Run id:

- `run_1779254646170_94392_0`

Result:

```text
status: completed
output: Observed 1 window(s) for 网易云音乐; frontmost app is ChatGPT Atlas.
```

Artifact evidence:

```text
appSelector=com.netease.163music
matchStrategy=bundle-id-exact
resolvedAppBundleId=com.netease.163music
resolvedAppName=网易云音乐
windowCount=1
window	网易云音乐	79097	com.netease.163music	3178	0		226	101	1060	752
```

Full pass rerun directory:

```text
.auv/app-passes/2026-05-20-netease-selector-regression-bf873de
```

Rerun commands:

```bash
cargo run --quiet -- app probe com.netease.163music \
  --output-dir .auv/app-passes/2026-05-20-netease-selector-regression-bf873de/probe

cargo run --quiet -- app analyze \
  .auv/app-passes/2026-05-20-netease-selector-regression-bf873de/probe

cargo run --quiet -- app distill \
  .auv/app-passes/2026-05-20-netease-selector-regression-bf873de/probe/analysis.json \
  --output-dir .auv/app-passes/2026-05-20-netease-selector-regression-bf873de/distill

cargo run --quiet -- app validate \
  .auv/app-passes/2026-05-20-netease-selector-regression-bf873de/distill
```

Rerun run ids:

- probe: `run_1779254667630_94512_0`
- analyze: `run_1779254691857_94979_0`
- distill: `run_1779254692300_94980_0`
- validate: `run_1779254692749_94978_0`

The rerun changed the important window evidence:

- `window_context.observed_window_count = 1`
- `window_context.primary_window_bounds = "226,101,1060,752"`
- `annotation_candidates[0].candidate_id = "window-primary-region"`
- `annotation_candidates[0].source = "window"`
- `annotation_candidates[0].evidence_step_id = "observe-windows"`

The distill and validate stages still preserved annotation linkage:

- `suggested_annotation_ids = ["window-primary-region"]`
- `used_annotation_ids = ["window-primary-region"]`
- `verification_mode = "evidence-only"`
- `unresolved_inputs = []`

Conclusion:

The original selector/window blocker is resolved for this live regression. The
NetEaseMusic output is still not promotable as a semantic skill because the
candidate remains a window-level pointer activation with evidence-only
verification.

## Decision

Do not promote this NetEaseMusic output as a skill.

The pass proves:

- V2 can carry `suggested_annotation_ids` through distill
- V2 can record `used_annotation_ids` during validate
- evidence-only validation is correctly separated from machine-asserted
  semantic success

The pass does not prove:

- NetEaseMusic search-entry grounding
- NetEaseMusic result-row grounding
- list scrolling
- song or playlist selection
- playback verification

Recommended next cut:

```text
tighten candidate taxonomy / OCR truth levels
```

The selector/window regression should stay covered by automated tests and live
evidence, but it is no longer the first blocker in this pass.

The remaining issue is truth-level precision: analysis still treats raw
low-confidence OCR observations as enough to make `ocr_sample_status` sound like
a candidate, while the active probe summary reports zero filtered matches under
the configured threshold. V2 should explicitly separate:

- raw OCR observation count
- filtered OCR match count
- visible text anchors
- list/result candidates
- semantic result evidence

After that, rerun this same pass again. If no list/result candidate appears, the
next real product cut is list row discovery. If row candidates appear but clicks
are unstable, the next cut is `alignAndClick`. If clicks work but success cannot
be proven, the next cut is verification provider hardening.
