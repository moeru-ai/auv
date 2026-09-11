# Balatro Ground-Truth Dataset Evidence

Date: 2026-08-04

## Evidence level

This note records live behavior on one development host. It proves that an
AUV-owned Balatro mod can stage exact card identities, capture synchronized
labels and frames, restore the original hand, and align those labels with a
separate `auv-driver-linux` screen capture. It is not a production support,
recognition-accuracy, or cross-environment claim.

The prototype source and generated corpus are local scratch material under
`docs/notes/neko/balatro-dataset-mod-prototype/`. They are intentionally not
committed as durable project source or redistributable game assets.

## Live environment

- Host: `neko-gpu-1`, Debian with Flatpak Steam and Proton.
- Balatro: `1.0.1o-FULL`.
- Lovely: `0.9.0`; installed `version.dll` SHA-256
  `ccfed59e4d245b7802c684fc86708e0a937f584d6e07d1ecc11e8eae22f9fc1a`.
- Steamodded: release `1.0.0~BETA-1814a-STEAMODDED`.
- BalatroBot: [`coder/balatrobot` commit
  `e7c6db8a9ad88318f6e4128eefd6e61aafc94885`](https://github.com/coder/balatrobot/tree/e7c6db8a9ad88318f6e4128eefd6e61aafc94885).
- AUV dataset mod: `0.3.0-prototype`, an independent Steamodded extension that
  uses BalatroBot only for its loopback JSON-RPC dispatcher.
- Steam per-application launch option:
  `WINEDLLOVERRIDES="version=n,b" %command%`.

Before installation, the entire Balatro save/profile directory and Steam's
per-user configuration were copied to:

```text
/home/neko/.local/state/auv-balatro-dataset-backups/20260804-205646/
```

The existing unreleased Steamodded checkout was preserved in the same backup.

## Prototype boundary

The mod registers seven narrow methods:

- `dataset.health` reports versions, state, render dimensions, and readiness.
- `dataset.catalog` reports the exact vanilla enhancements, editions, seals,
  facings, contrast modes, and deck backs that the current game exposes.
- `dataset.status` reports the current typed stage and signature for settlement
  polling.
- `dataset.stage_cards` validates and stages typed per-card identity and visual
  fields.
- `dataset.stage` saves the visible hand and replaces its card bases with an
  exact requested sequence; it remains the base-card convenience endpoint.
- `dataset.capture` freezes `love.update`, captures the framebuffer, and reads
  labels inside the `love.graphics.captureScreenshot` callback.
- `dataset.restore` restores the saved card bases, including on collector
  cleanup.

Each card record includes rank, suit, card key, instance IDs, face direction,
highlight/debuff state, seal, edition, enhancement, target transform `T`, and
visible transform `VT`, and identity visibility. Version 0.3 accepts exact
vanilla enhancement, edition, seal, debuff, highlight, facing, contrast, and
deck-back fields. Each sample also records game/mod versions, render
dimensions, tile/canvas scale, room transform, and language.

Deck-back staging is visual-only at the card-instance boundary: the mod updates
the existing back Sprite position and never calls `Back:change_to` or mutates
`G.GAME.selected_back`. Negative edition is likewise staged as a visual-only
shader record because the normal playing-card API changes hand capacity.
Blind updates can recalculate debuff state, so status and capture entrypoints
re-enforce the validated instance-bound visual spec before signing a frame.

Frame paths are constrained beneath `auv-dataset/frames`; sidecar manifests
are written beneath `auv-dataset/manifests`. The prototype exposes no arbitrary
Lua execution and does not copy upstream endpoint or gameplay code.

## Generated samples

The first live run used seed `AUVTRAIN`, red deck, white stake, and a stable
eight-card hand in `SELECTING_HAND`.

| Session | Frames | Card records | Unique rank/suit faces | Position coverage | Hidden cards |
| --- | ---: | ---: | ---: | --- | ---: |
| `base-52-live-20260804` | 7 | 56 | 52 | Ordered proof pass | 0 |
| `base-52-shuffled-r4-20260804` | 28 | 224 | 52 | Each face in 2-5 screen positions | 0 |
| `base-52-shuffled-r40-20260804` | 280 | 2,240 | 52 | Forty independently shuffled rounds | 0 |

The shuffled corpus contains at least four examples of every base face. Some
faces occur six times because each 52-card round must fill the last eight-card
frame. All internal captures were `2043x1126`; the shuffled local corpus is
approximately 8.8 MiB.

Visual inspection of the first ordered frame confirmed that the rendered hand
matched its labels (`H2` through `H9`). After collection, `dataset.restore`
reported eight restored cards, and `dataset.health` reported no active staged
hand or pending capture.

## Project AIRI-compatible dataset export

The 40-round capture was exported inside the existing Linux checkout at:

```text
/home/neko/Git/github.com/proj-airi/game-playing-ai-balatro/
  data/datasets/games-balatro-2024-mod-ground-truth-prototype/
```

The independent exporter prototype is
`cli/prototypes/export_mod_ground_truth.py` in that checkout. Its output is
approximately 221 MiB and contains:

- 280 hand-region YOLO images with 2,240 `poker_card_front` objects, preserving
  the existing Project AIRI entity class ID `6`.
- 2,080 rectified full-card crops and 2,080 top-left rank/suit crops.
- Exact 52-class labels, with 40 balanced samples for every rank/suit identity.
- Full source frames, frame hashes, stage-token hashes, game/Mod/render
  versions, source and ROI geometry, quadrilaterals, axis-aligned boxes, and
  modifier fields.
- Deterministic complete-round splits: 28 train groups / 1,456 card crops, six
  validation groups / 312 crops, and six test groups / 312 crops.

The four cards repeated to fill the final eight-card frame in each round stay
in object detection but are excluded from classification, producing exact
class balance. Complete rounds are the split unit, so frames, crops, and these
padding duplicates cannot cross train/validation/test boundaries.

Full frames contain other visible entities for which the current Mod does not
produce labels. The primary YOLO view therefore crops to the exact union of
the eight annotated hand-card quadrilaterals. Full frames remain as provenance,
but are not presented as completely annotated detector inputs. This prevents
unlabelled deck, stack, or tooltip entities from becoming false-negative
training examples.

Validation completed with the repository's Pixi environment:

- Ruff formatting and lint passed for the exporter.
- Ultralytics resolved all train, validation, and test paths with ten class
  names from both the embedded-repository and standalone YAML configurations.
- Every detector image has one label file and every crop has one metadata row.
- All 2,240 YOLO rows use class ID `6` and valid normalized coordinates.
- Every split contains all 52 card classes with equal per-class counts.
- Visual overlays align the Mod `VT` quadrilaterals with the rendered card
  borders; sampled full-card and corner crops retain the expected identity.

Three superseded local exports were moved, not deleted, to:

```text
/home/neko/.local/state/auv-balatro-dataset-prototypes/20260804/
```

They record why full-frame partial labels, padded hand regions, and a
repository-root path ambiguity were rejected.

## Playing-card visual-variant dataset

Mod 0.3 generated a separate card-specialized raw session:

```text
/home/neko/.var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/
  compatdata/2379780/pfx/drive_c/users/steamuser/AppData/Roaming/Balatro/
  auv-dataset/collections/card-variants-v3-20260804.jsonl
```

It contains 819 synchronized frames, 6,552 card records, 117 complete-round
groups, and 47 profiles. Every single-axis profile uses three independently
shuffled complete 52-card rounds, one per split. The exact vanilla axes are:

- all 52 rank/suit identities;
- base plus eight enhancements;
- four editions and four seals;
- debuff, highlight, front/back, and both contrast settings;
- all fifteen vanilla deck backs.

Twelve additional deterministic pairwise profiles exercise modifier
interactions. This is not a full Cartesian product; the omission is explicit
in the collector because exhaustive combinations would be mostly duplicate
framebuffer data without an error-analysis trigger.

The separate exported dataset is:

```text
/home/neko/Git/github.com/proj-airi/game-playing-ai-balatro/
  data/datasets/games-balatro-2024-playing-card-ground-truth-prototype/
```

The 1.3 GiB output has 18,421 files. Each split has 39 round groups, 273
detector frames, 2,184 typed objects, and 2,184 rectified all-variant crops.
The detector projection contains 1,344 `poker_card_front` and 840
`poker_card_back` instances per split. Separate views contain:

| Split | Visible identity crops | Corner crops | Deck-back crops |
| --- | ---: | ---: | ---: |
| train | 1,172 | 1,172 | 780 |
| validation | 1,173 | 1,173 | 780 |
| test | 1,173 | 1,173 | 780 |

Every split has all 52 visible identity classes with 21-23 samples per class,
and exactly 52 non-padding crops for each of the fifteen deck-back classes.
Stone and face-down instances stay in detection and the all-variant view but
have `identity_visible=false`; they are excluded from identity and corner
supervision.

The exporter is
`cli/prototypes/export_card_variants.py` in the Project AIRI checkout. Ruff
formatting and lint passed. Both repository-embedded and standalone
Ultralytics YAML files resolved train/validation/test successfully. Automated
checks also proved:

- one image, label file, and metadata row per detector frame;
- one metadata row per crop in all four derived views;
- valid normalized YOLO coordinates using only existing entity IDs 4 and 6;
- no round-group overlap across splits;
- every one-axis profile present in every split;
- exact fifteen-way back-label balance and complete 52-class identity presence.

Visual inspection covered the geometry overlay and a 35-profile single-axis
contact sheet. All enhancements, editions, seals, debuff, contrast, highlight,
and deck-back pixels matched their typed labels.

Two earlier raw sessions remain as evidence but are excluded from the final
dataset: `card-variants-v1-20260804` stopped at 357 frames when status exposed
blind-recalculated debuffs, and `card-variants-v2-20260804` completed 525
frames before deck-back staging existed. The v2 export was moved intact to:

```text
/home/neko/.local/state/auv-balatro-dataset-prototypes/20260804/
  card-variants-v2-without-all-backs/
```

The first edition implementation also exposed why normal negative-edition
staging is invalid for corpus generation: it changed hand capacity and dealt a
ninth card. The final Mod uses visual-only negative shader state, re-enforces
debuff at status/capture boundaries, and restores the original eight-card hand.

## Card-detector training probe

The Project AIRI training entrypoint was changed from a hard-coded experiment
to a parameterized CLI while retaining its previous defaults. Focused tests
cover default compatibility, argument overrides, and automatic device
selection. Ruff formatting/lint and the three tests passed in the repository's
Pixi environment.

The existing Balatro entity detector was fine-tuned on the card-specialized
dataset rather than starting from generic YOLO11n weights. The deterministic
run used image size 640, batch 64, seed `20260804`, and early-stopping patience
20 on an RTX 4080 SUPER. It stopped after epoch 63 and selected epoch 43. The
packaged local artifacts are:

```text
/home/neko/Git/github.com/proj-airi/game-playing-ai-balatro/
  models/games-balatro-2024-yolo-card-detection-mod-ground-truth/
```

On the group-held-out 273-image / 2,184-object test split, the source model had
precision `0.608135`, recall `0.533036`, `mAP50=0.617639`, and
`mAP50-95=0.400004`. The fine-tuned model had precision `0.999943`, recall
`1.0`, `mAP50=0.995`, and `mAP50-95=0.994965`. Both supervised classes,
`poker_card_back` and `poker_card_front`, were approximately `0.995` mAP50-95.

This high score is an in-session dataset result, not deployment accuracy. The
groups do not overlap, but every split came from the same game session and
render setup. A full-frame visual probe found all eight hand cards and the deck
back but also five false-positive front-card boxes on the left-side blind/score
UI at confidence `0.25`; multiple false positives remained above `0.8`. This
matches the dataset contract: its detector projection contains fully annotated
hand-region crops, while full frames retain incompletely labelled UI entities
only as provenance. The model therefore requires a hand-region crop today.
Full-frame use needs a separate owner-approved corpus with complete UI labels
or explicit hard-negative supervision.

The fixed-shape opset-19 ONNX export passed `onnx.checker` and ONNX Runtime CPU
inference. On a held-out card-back image, PyTorch and ONNX both returned the
same eight class-4 detections. Model hashes and exact metrics are recorded in
the package's `metrics.json`. Dataset/source-weight/game-asset redistribution
licensing remains unreviewed, so the package is local experimental evidence.

The dataset and model were subsequently uploaded as private Hugging Face
repositories, preserving that licensing boundary:

- `proj-airi/games-balatro-2024-playing-card-ground-truth`
- `proj-airi/games-balatro-2024-yolo-card-detection-mod-ground-truth`

AUV now accepts an opt-in `--cards-model` ONNX path. This does not replace the
global entities detector: observation runs the specialized model only on the
normalized hand band, maps detections back to source-frame pixels, and uses
them as the hand-slot source while retaining global entity/UI results for all
other state. The initial no-crop integration reproduced the expected failure
on a 2043x1126 full frame: thirteen hand slots were emitted from eight hand
cards, the deck back, and four UI false positives. With the specialized crop,
the same AUV observation emitted exactly eight left-to-right hand slots at
confidence `0.9613-0.9731`; global entities still supplied phase evidence.
Focused crop/remapping and source-selection regression tests passed together
with all 22 library tests.

A later Linux live probe exposed a second coordinate boundary: window capture
was unavailable, so the portal returned a `2560x1440` full display containing a
roughly `2048x1129` Balatro viewport offset from the desktop origin. Applying
the hand percentages to the display produced eleven slots. The observation
crop now downsamples boundary samples, converts them to grayscale, thresholds
edge contrast, and combines the two long-edge scores with the corpus viewport
aspect ratio before deriving the hand band. On the recorded live frame it
resolved `viewport=(512,311,2048,1129)` and
`hand=(1054,898,1127,361)`. The same remote detector invocation then emitted
exactly eight hand slots at confidence `0.9659-0.9723`. A synthetic offset-
viewport regression, including a stronger internal panel edge, passed with all
23 library tests. Arbitrary floating-window recovery remains explicitly
deferred at the detector call site; this evidence covers the reproduced
right/bottom-clipped display fallback.

Temporary stage timing on one live invocation attributed the approximately
29-second wall time as follows: class-asset resolution `4.22s`, window resolve
plus portal capture `7.46s`, OCR `8.44s`, entities RPC `4.39s`, cropped card RPC
and crop `0.15s`, and UI RPC `2.86s`. Two fixed-frame warm probes put the full-
frame entities/UI RPCs at `3.76-5.15s` and `3.70-6.99s`, while the cropped card
path remained `0.12-0.16s`. The card model and viewport preprocessing are not
the latency bottleneck; serial OCR plus repeated uncompressed full-frame RPCs
are.

A follow-up optimization tested concurrent full-frame OCR and detection first.
That reduced one occluded live invocation only from `29.03s` to `26.26s`
because the large RPCs competed for the same remote transport. The accepted
path instead waits for UI detection, skips OCR when no numeric UI was detected,
and otherwise derives one padded numeric-UI crop on the caller. It updates the
derived `Capture.bounds` before RPC delivery, preserving screen-coordinate
projection while avoiding a second full-display RGBA transfer. On a visible
live frame this sent a `456x856` crop instead of `2560x1440` and reduced the OCR
stage from `8.44s` to `0.62s`. Observed end-to-end time was `17.25-21.88s`; the
spread came primarily from portal capture, which reached `9.80s` in the slower
probe. Remaining measured costs were class-asset resolution `2.15s` and the
three detector RPCs `4.85s`.

Linux OCR is Tesseract through `leptess`. The driver currently PNG-encodes each
request and constructs a new language-configured engine. Engine reuse was not
added in this slice: the complete cropped OCR RPC is now about `0.62s`, while
portal capture and full-frame entities/UI transport are materially larger. A
driver-owned reuse cache also needs a thread/concurrency lifecycle rather than
an observation-local mutex.

## Independent driver-domain alignment

To check that game-internal labels describe what AUV sees through its actual
Linux display driver, the mod staged:

```text
S_A, H_K, C_Q, D_J, S_T, H_9, C_8, D_7
```

Without changing that stage token, `auv-driver-linux` captured the external
portal display:

- Run: `fbfeb64f-6dbc-a6dc-6ab3-cc93a86cf9e1`.
- Artifact:
  `auv://runs/fbfeb64f-6dbc-a6dc-6ab3-cc93a86cf9e1/artifacts/019fcce8-33a0-7cc2-90bd-644bb254fcd9`.
- PNG SHA-256:
  `cd4667e652fdb41e6519cebb55835f4ab27b9c425b1f79e75aab01cb6c8e5bcb`.
- Capture backend and dimensions: portal, `2560x1440`.

Visual inspection showed exactly A-spades, K-hearts, Q-clubs, J-diamonds,
10-spades, 9-hearts, 8-clubs, and 7-diamonds. The current AUV visual state
reader found eight `poker_card_front` regions with confidence from about
`0.9397` to `0.9578` and no diagnostics. A subsequent same-token internal
capture returned the same eight exact labels, after which restoration
succeeded.

This proves alignment for that staged hand. It does not yet measure exact
rank/suit classification accuracy: the current visual result identifies card
front regions, while the Mod supplies the exact ground truth.

## Installation findings

- Use the official per-game Steam launch option for Proton injection. A global
  Wine registry override has broader effects than this development tool needs.
- When transferring mod archives from macOS, suppress AppleDouble metadata.
  `._*.toml` files were treated as Lovely patch files and failed UTF-8 parsing.
  The generated metadata files were removed from the target after their exact
  paths were enumerated.
- The Lovely console is visible in the external full-display screenshot. It
  does not overlap the staged cards, but future driver-domain collection should
  hide it or capture/crop the Balatro window explicitly.

## Known boundaries and next slice

- Both the 280-frame base corpus and 819-frame visual-variant corpus use the
  Mod's same-callback framebuffer capture. The external-driver check is one
  proof frame, not yet the collection transport.
- There is no durable `prepare -> external capture -> commit` protocol yet.
  That handshake must freeze a stage, carry a shared sample ID into the AUV run
  artifact, verify the unchanged stage signature, and then restore/unfreeze.
- The specialized corpus exhausts each supported vanilla card axis separately,
  but interaction coverage is pairwise rather than Cartesian. It still uses
  one resolution, language, game version, run, hand layout, and background
  family. Motion phases, deliberate overlap variation, multiple display
  scales, localization variants, and modded card assets remain outside this
  prototype.
- The current split is grouped by complete shuffled round, but all rounds still
  share one game run, seed, language, resolution, and background family. Final
  evaluation must reserve independent sessions and driver-domain captures.
- Redistribution of rendered game assets or derived weights needs a separate
  rights review; upstream source licenses do not decide game-asset rights.

The smallest useful follow-up is an AUV-owned external capture handshake and
independent-session collection. The crop/manifest exporter now exists; moving
its input to the production driver domain would make a real rank/suit accuracy
evaluation possible.
