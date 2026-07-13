# AUV Skill Contract v0

Date: 2026-05-15

Status: provisional contract distilled from the current QQ音乐 macOS skill
slice

## Purpose

This note captures the first generic skill contract that is actually backed by
running code, not by architecture fantasies.

It is extracted from the current QQ音乐 narrow-skill productization work:

- `open_search_submit_query`
- `search_ocr_anchor`
- `play_visible_anchor`

The point is to define only the fields and behaviors that are already proven
useful across those slices.

## Manifest Shape

Current executable skill manifests use:

- `recipe_id`
- `version`
- `status`
- `platform`
- `target_app`
- `strategy`
- `objective`
- `inputs`
- `preconditions`
- `disturbance_policy`
- `steps`
- `verification`
- `known_limits`

This is the current minimum viable product contract. Do not add new top-level
sections unless a second app or platform actually needs them.

## Strategy Contract

Phase 2 starts by turning the most important recipe truth into explicit schema
instead of prose:

- `strategy.family`
- `strategy.grounding`
- `strategy.activation`
- `strategy.verificationContract`

This is the first structured statement of what a narrow skill actually is.

Current validated values are intentionally narrow:

- `playback`
- `result-selection`
- `search-entry`
- `native-text`

Current allowed strategy combinations are also intentionally narrow:

- `search-entry / ax-text-input / clipboard-submit / captureEvidence`
- `result-selection / ocr-anchor / pointer-click / captureEvidence`
- `playback / ocr-anchor / pointer-double-click / verifyImageText`
- `playback / visual-row / pointer-row-activation / verifyNowPlayingTitle`
- `native-text / ax-text / pointer-focus-clipboard-paste / verifyAxText`

And the important point is not the exact labels. The point is that a bundle or
reviewer should no longer need to infer whether a skill is:

- OCR-anchor grounded
- row-fallback grounded
- AX-text grounded
- evidence-image verified
- AX now-playing verified

from scattered step prose.

The same strategy truth should also survive bundle/package export. Otherwise the
repo is honest but the distillation product is not.

As of 2026-05-18, this truth now survives export in two machine-consumable
forms:

- structured `strategy.*` fields
- normalized `taxonomyId`

The current `taxonomyId` shape is:

`family.grounding.activation.verification-contract`

## Step Contract

Each step currently needs:

- `id`
- `command_id`
- optional `disturbance`
- optional `expect`
- optional `args`

Steps execute through the shared runtime, not through ad-hoc tool bindings.

Current productized narrow skills should use `expect` whenever a later human or
model would otherwise incorrectly read "command completed" as "skill
succeeded". The first concrete use is QQ音乐 playback verification:

- fail if OCR resolves zero matches
- fail if the expected title string is absent
- fail if the evidence-capture step produces no artifact

## Variable Substitution Contract

Current manifests may reference:

- input variables such as `${query}`
- target app variables such as `${app_id}`
- prior-step result variables such as `${step_capture_evidence_artifact_image_0}`

The runner currently exports at least:

- `step_<id>_run_id`
- `step_<id>_status`
- `step_<id>_output`
- `step_<id>_artifact_count`
- `step_<id>_artifact_<n>`
- `step_<id>_artifact_last`
- `step_<id>_artifact_image_0`
- `step_<id>_artifact_image_last`

This is the first generic bridge that lets one step verify evidence produced by
another step without dropping to shell-specific parsing.

## Disturbance Contract

Every skill must declare:

- `disturbance_policy.max_disturbance`
- `disturbance_policy.declared_classes`

Every step must declare:

- `disturbance.classes`
- `disturbance.max`

The runner must reject execution when a step exceeds the allowed disturbance
budget.

## Runtime Contract

`auv-cli skill run` is now the product-facing execution entrypoint.

The runner must:

- resolve the skill from the `recipes/` catalog or from an explicit path
- validate disturbance policy
- serialize live-desktop app access through a per-app lock
- replay steps through the shared runtime
- surface run ids and artifact paths
- export prior-step artifact paths into later step variables

Once a narrow skill has more than one validated case, `auv-cli skill cases run`
becomes the product-facing coverage entrypoint. That is the current QQ音乐
shape: one executable recipe, plus one case matrix that declares which inputs
are actually validated.

Case matrices may also need to separate:

- the user-requested semantic target
- the concretely verified observed target

The first current example is QQ音乐 row fallback on a Chinese result page,
where `requested_title` and `target_title` can diverge. That is not a cosmetic
detail. It is the difference between "activation path validated" and
"requested-title selection validated".

## Verification Contract

Productized narrow skills should define:

- `expected_signals`
- `success_criteria`
- `non_goals`

This is not decoration. It is what stops a validated narrow skill from being
marketed as a generalized app capability.

## What Is Not Generic Yet

The following are still app-specific or slice-specific:

- QQ音乐 row double-click as the activation strategy
- the current player-title OCR verification region
- the current anchor texts and query baselines
- the exact relationship between one query and one expected title

Those belong in skill inputs, case matrices, or app-specific docs, not in the
generic contract.
