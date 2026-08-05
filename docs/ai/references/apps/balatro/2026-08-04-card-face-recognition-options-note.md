# Balatro card-face recognition options

Date: 2026-08-04

Status: research note. This is not approval to add a new recognition surface.

## Conclusion

AUV should not start with a generic 52-class playing-card detector. The hand
slots and card boxes are already localized, so the shortest path is to crop the
Balatro corner index and classify **13 ranks plus 4 suits**, with rejection and
temporal consensus.

The strongest first candidate already exists: Project AIRI's
[`games-balatro-2024-card-corner-classifier`](https://huggingface.co/proj-airi/games-balatro-2024-card-corner-classifier).
It is Balatro-specific, MIT-licensed, exports a 2.77 MB ONNX model, and uses the
same two-head shape that this task needs. Standard-card models remain useful as
baselines or training-pipeline references, but their reported metrics do not
transfer to Balatro.

## Shortlist

| Priority | Candidate | Evidence and fit | Decision |
|---|---|---|---|
| 1 | [Project AIRI Balatro card-corner classifier](https://huggingface.co/proj-airi/games-balatro-2024-card-corner-classifier) | Balatro crops; `float32[N,3,64,64]` input; 13-rank and 4-suit logits; ONNX; MIT. The model card reports rank/exact accuracy `0.8387` and suit accuracy `1.0` on its recovered local validation run. | Evaluate first on a frozen AUV Linux screenshot holdout. Do not promote the reported validation number to a live capability claim. |
| 2 | [sroot/lgd-cards-gen3](https://huggingface.co/sroot/lgd-cards-gen3) | Current member of an actively documented YOLO11s family that detects 52 standard-card corner pips. The [gen4 card](https://huggingface.co/sroot/lgd-cards-gen4) identifies gen3 as the served model and reports roughly `0.85` normal-gameplay recall on its casino-video holdout. ONNX is available; AGPL-3.0. | Zero-shot comparison only. It solves full-frame physical-card detection, carries AGPL obligations, and has severe Balatro domain shift. |
| 3 | Balatro-specific normalized templates | [EdjeElectronics' OpenCV detector](https://github.com/EdjeElectronics/OpenCV-Playing-Card-Detector) demonstrates corner isolation followed by separate rank/suit templates and explicitly recommends templates captured from the target deck. | Keep as an interpretable baseline, but reimplement the small algorithm if selected: that repository has no published license or metrics and targets isolated physical cards on dark backgrounds. |
| 4 | BalatroBot as labeling oracle | [BalatroBot](https://github.com/coder/balatrobot) is an active MIT JSON-RPC mod exposing exact game state and controls. | Useful only to create or audit labeled screenshots if an owner approves a modded benchmark. It is not screenshot recognition and must not become evidence for the unmodified UI path. |

## Existing AUV seam

The repository already has a feature-gated ONNX adapter in
`supported/games/auv-game-balatro/src/card_corner.rs` and names the Project AIRI
model as the default asset in `config.rs`. The adapter's tensors and label
order match the model card. It is not yet connected to the production card-read
path: `cards read` still uses OCR, color-based suit inference, and diagnostic
deck-template rank candidates. Consequently, the current `0/168` live complete
card reads do **not** evaluate the new ONNX model.

The upstream training implementation is also available in
[`card_corner_classifier.py`](https://github.com/proj-airi/game-playing-ai-balatro/blob/main/src/ai_balatro/datasets/card_corner_classifier.py),
with an [ONNX export utility](https://github.com/proj-airi/game-playing-ai-balatro/blob/main/cli/export-card-corner-classifier-onnx.py).
The Hugging Face artifact is MIT, but the GitHub repository itself has no root
license file, so source-code reuse should be reviewed separately from consuming
the published model.

## Why ordinary playing-card models are not direct solutions

Ordinary-card datasets and detectors see printed physical decks, camera
perspective, and casino tables. Balatro introduces a different font and suit
rendering, overlapping animated cards, UI scaling, selected-card displacement,
and editions/effects such as foil, holographic, polychrome, seals, enhancement,
and debuff treatments. No standard-card model or dataset found in this review
claims Balatro support.

The limitation is visible even within the standard-card domain. The
[`lgd-cards-gen4` model card](https://huggingface.co/sroot/lgd-cards-gen4)
reports that it was reverted on the day of deployment after nested false reads
and missed hole cards; on a shared 318-frame holdout it reports recall `0.786`
and a precision proxy `0.752`. A
[`Poker_Detection`](https://github.com/zinuoli/Poker_Detection) project reports
YOLOv7 `mAP@0.5 = 0.959`, but its target is physical bridge cards and the root
repository has no unified license. These are useful architecture references,
not transferable Balatro accuracy claims.

## Data and algorithm references

- [`Lolitofdez55/playing-cards`](https://huggingface.co/datasets/Lolitofdez55/playing-cards)
  is an MIT synthetic dataset with four sets of 10,000 images, rank/suit
  concepts, class labels, and card-corner coordinates. It can bootstrap generic
  augmentation experiments but uses ordinary ornate cards, not Balatro.
- [`geaxgx/playing-card-detection`](https://github.com/geaxgx/playing-card-detection)
  is an MIT synthetic-data generator that rotates, scales, and overlaps cards
  while labeling the printed corners. Its augmentation pattern is reusable;
  its ordinary-card assets and labels are not a Balatro training set.
- [`masarunakajima/playing_card_detection`](https://github.com/masarunakajima/playing_card_detection)
  applies the same corner-label idea to YOLOv7 and documents generation of
  40,000 training and 2,000 validation images. It publishes neither weights,
  metrics, nor a license, so it is reference material only.
- The original [Project AIRI Balatro repository](https://github.com/proj-airi/game-playing-ai-balatro)
  remains the only reviewed GitHub project with a Balatro-specific rank/suit
  crop classifier and reproducible ONNX export path.

## Recommended evaluation slice

1. Freeze an offline holdout from AUV's recorded Linux frames and label exact
   rank/suit per slot. Split by run or scene, never adjacent frames, to avoid
   temporal leakage.
2. Cover every rank and suit plus selected/unselected positions, overlap,
   resolution/UI scale, and the supported visual effects. Report coverage
   separately when rare effects are absent.
3. Run the existing Project AIRI ONNX model on the already-known card-corner
   crops. Measure rank, suit, exact-card accuracy, rejection coverage, and a
   confusion matrix; keep `6/9`, `10`, and face-card confusions explicit.
4. Calibrate separate rank/suit thresholds and require 3--5 stable frames to
   agree before publishing a verified card. Preserve low-confidence logits as
   diagnostics rather than promoting them to `rank` or `short_code`.
5. If the first model misses the gate, fine-tune the same small two-head model
   on AUV-labeled Balatro crops. Compare it against normalized edge/template
   matching before considering a larger 52-class detector.

This slice would answer the actual product question: exact-card accuracy on
Balatro under AUV capture. It avoids conflating entity/slot detection,
driver delivery, ordinary-card benchmarks, and verified card content.
