# AUV Working Agent Notes

Date: 2026-05-17

This file is the local working context for the current AUV effort.
It is not the repository-wide policy file. `AGENTS.md` stays authoritative for
repo rules.

## Proven So Far

- QQ音乐 is a validated narrow playback slice.
- `verifyNowPlayingTitle` is the stable AX-based verification contract for that slice.
- `verifyAxText` is the generic AX text verification contract for native apps with reliable text areas.
- Notes is validated as a native-app sample without screenshot OCR.
- TextEdit is validated as a native-app sample without screenshot OCR.

## Current Shape

- Shared runtime primitives now include `debug.verifyAxText`.
- The native-app skill tree is documented in `docs/ai/references/2026-05-17-auv-native-app-skill-tree.md`.
- The distillation template is documented in `docs/ai/references/2026-05-17-distillation-template-v0.md`.
- The current validated sample set is QQ音乐, Notes, and TextEdit.
- The reusable boundary is "validated sample -> controlled distillation candidate", not "keep chasing OCR".
- The first bundle-shaped artifact is `bundles/native-app-skill-tree.v0.json`.

## Working Rule

Do not keep re-proving the same chain.
Use the validated sample set to shape candidate skill bundles, contract comparisons,
and regression coverage.

## Next Step

Controlled distillation.
If model quota is spent, spend it on candidate skill artifacts and contract
comparison across the validated samples.
