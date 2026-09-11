# Project AIRI Balatro entities dataset structure research

Date: 2026-08-04

Status: primary-source review of the published dataset and its owning repository.
This note describes the current artifact; it is not an AUV support claim and
does not approve publishing Balatro screenshots.

## Executive finding

The existing Project AIRI dataset is useful as a compact distribution pattern:
it keeps full-frame images, YOLO labels, a class list, and the original Label
Studio export together, then pins the Hugging Face repository as a submodule of
the owning project. It is **not** a card-face dataset: `poker_card_front` is one
of ten broad entity classes, and the published labels do not contain rank,
suit, edition, enhancement, or seal.

An AUV Mod-ground-truth dataset should retain the useful packaging ideas but
replace the identity, provenance, split, and annotation contracts. The most
important reason is concrete: 335 Label Studio tasks collapse to 286 published
images because the exporter flattens source paths to colliding basenames. That
also reduces 2,697 source rectangles to 2,423 published boxes.

## Reviewed revisions and ownership

The current Hugging Face dataset revision is
`a3fcce53757fc0ea0c10bd2a178f612c631a6173`. The owning
[`proj-airi/game-playing-ai-balatro`](https://github.com/proj-airi/game-playing-ai-balatro)
repository embeds that dataset at
`data/datasets/games-balatro-2024-entities-detection`; its
[`gitmodules`](https://github.com/proj-airi/game-playing-ai-balatro/blob/9654de554e68ea3367bffbb5574b84563423603d/.gitmodules)
points directly to the Hugging Face Git repository. The local checkout reviewed
for the owner-side tooling was commit
`d23013f8e41e592b5964a1d7468e065bc6d008e1`; the relevant files are unchanged
at remote `main` commit `9654de554e68ea3367bffbb5574b84563423603d`.

The
[`dataset card`](https://huggingface.co/datasets/proj-airi/games-balatro-2024-entities-detection/blob/a3fcce53757fc0ea0c10bd2a178f612c631a6173/README.md)
names Neko Ayaka and RainbowBird as annotation creators, classifies the task as
object detection, and declares CC BY-SA 4.0. There is no separate license file
or per-image rights/provenance record in the dataset revision. In particular,
the dataset-card declaration alone does not document the rights boundary for
the underlying Balatro imagery, so AUV should treat publication of raw game
screenshots as a separate review from internal data generation.

## Exact published layout

The pinned
[`repository tree`](https://huggingface.co/datasets/proj-airi/games-balatro-2024-entities-detection/tree/a3fcce53757fc0ea0c10bd2a178f612c631a6173)
contains:

```text
data/
  train/
    metadata.jsonl                 # 286 rows
    yolo/
      images/                      # 286 JPG images
      labels/                      # 286 YOLO TXT files
      classes.txt
      notes.json
      project.label-studio.json    # original annotation export
  val/
    yolo/
      images/                      # 7 images, no labels or metadata.jsonl
docs/
  cover.png
  example-1.png
  example-2.png
```

Hugging Face's current
[`datasets-server info`](https://datasets-server.huggingface.co/info?dataset=proj-airi%2Fgames-balatro-2024-entities-detection)
loads the repository as `imagefolder` with only two features:

```text
image: Image
label: string
```

It reports 286 train examples and zero validation examples. The seven files
under `data/val/yolo/images` therefore do not form a usable published
validation split. The train image dimensions in the pinned revision are:

| Resolution | Images |
| --- | ---: |
| `1280 x 660` | 80 |
| `1920 x 884` | 75 |
| `2622 x 1206` | 131 |

All seven unlabeled `val` files are `1280 x 660`.

Each row of
[`metadata.jsonl`](https://huggingface.co/datasets/proj-airi/games-balatro-2024-entities-detection/blob/a3fcce53757fc0ea0c10bd2a178f612c631a6173/data/train/metadata.jsonl)
has this shape:

```json
{
  "file_name": "yolo/images/out_00001.jpg",
  "label": "6 0.5 0.4091737739 0.1022297013 0.2749582489"
}
```

`label` is unparsed, possibly multiline YOLO text. Every annotation line is
`class_id center_x center_y width height` with normalized coordinates. The
published Hugging Face row does not carry structured boxes, image dimensions,
source/session identity, hashes, annotation author, capture configuration, or
game/mod versions. Seven train rows have an empty label string, so they are
negative/empty-box examples rather than missing rows.

## Classes and observed distribution

The ten class IDs are defined consistently by
[`classes.txt`](https://huggingface.co/datasets/proj-airi/games-balatro-2024-entities-detection/blob/a3fcce53757fc0ea0c10bd2a178f612c631a6173/data/train/yolo/classes.txt),
[`notes.json`](https://huggingface.co/datasets/proj-airi/games-balatro-2024-entities-detection/blob/a3fcce53757fc0ea0c10bd2a178f612c631a6173/data/train/yolo/notes.json),
and the owner's
[`v2 entity configuration`](https://github.com/proj-airi/game-playing-ai-balatro/blob/9654de554e68ea3367bffbb5574b84563423603d/configs/v2-balatro-entities/dataset.yaml).
Counts below were calculated from the pinned raw artifacts. “Source” means the
Label Studio export; “published” means the 286 YOLO files and `metadata.jsonl`.

| ID | Class | Published boxes | Images containing class | Source boxes |
| ---: | --- | ---: | ---: | ---: |
| 0 | `card_description` | 86 | 73 | 92 |
| 1 | `card_pack` | 141 | 75 | 163 |
| 2 | `joker_card` | 685 | 191 | 750 |
| 3 | `planet_card` | 160 | 95 | 193 |
| 4 | `poker_card_back` | 56 | 36 | 66 |
| 5 | `poker_card_description` | 20 | 20 | 22 |
| 6 | `poker_card_front` | 906 | 125 | 995 |
| 7 | `poker_card_stack` | 252 | 248 | 298 |
| 8 | `spectral_card` | 26 | 7 | 26 |
| 9 | `tarot_card` | 91 | 53 | 92 |
| | **Total** | **2,423** | | **2,697** |

This distribution is strongly imbalanced. It can support a broad entity
detector, but it cannot supervise exact card-face recognition because every
visible playing card shares class 6.

## Provenance recovered from the Label Studio export

The retained
[`project.label-studio.json`](https://huggingface.co/datasets/proj-airi/games-balatro-2024-entities-detection/blob/a3fcce53757fc0ea0c10bd2a178f612c631a6173/data/train/yolo/project.label-studio.json)
contains 335 annotated tasks and 2,697 manual rectangle results. Their local
source paths identify three capture cohorts:

| Source cohort | Tasks |
| --- | ---: |
| `luoling8192-2025-09-17-23-02-diff` | 113 |
| `luoling8192-2025-09-19-21-39-diff` | 147 |
| `nekomeowww-2025-09-19-21-54-diff` | 75 |

Those task paths preserve recorder/session hints inside an annotation-tool
snapshot, but the published row schema discards them. It also exposes local
directory names without turning them into a stable, documented provenance
contract.

### Reproduced basename collision

The 335 tasks have exactly 286 unique source basenames. Forty-seven basenames
occur more than once, accounting for 49 excess task rows. For example, multiple
capture cohorts can each contain `out_00042.jpg`.

The owner's
[`Label Studio to YOLO exporter`](https://github.com/proj-airi/game-playing-ai-balatro/blob/9654de554e68ea3367bffbb5574b84563423603d/cli/label-studio/export-yolo.py)
strips the source path to its basename and copies all images into one directory;
it similarly strips converter prefixes when renaming labels. Colliding frames
and labels can therefore overwrite each other. The published difference is:

```text
335 source tasks   - 49 colliding task rows = 286 published images
2697 source boxes  - 274 overwritten boxes = 2423 published boxes
```

This is direct evidence that the next format needs globally stable sample IDs
and collision rejection, not only prettier folder naming.

## Existing owner-side toolchain

The documented and implemented path is:

```text
game-record frame folders
  -> Label Studio manual rectangles
  -> cli/label-studio/export-yolo.py
  -> images/ + labels/ + classes.txt
  -> cli/export-datasets.py
  -> data/<split>/yolo/* + metadata.jsonl
  -> Hugging Face imagefolder repository
```

The owning repository says the model used fewer than 1,000 manually labeled
images with YOLO11n as its base in its
[`README`](https://github.com/proj-airi/game-playing-ai-balatro/blob/9654de554e68ea3367bffbb5574b84563423603d/README.md).
The
[`Hugging Face exporter`](https://github.com/proj-airi/game-playing-ai-balatro/blob/9654de554e68ea3367bffbb5574b84563423603d/cli/export-datasets.py)
copies the YOLO layout and embeds each TXT file as one string; it does not
validate duplicate IDs, image/label pairing, coordinates, class ranges,
checksums, provenance, or split leakage.

The checked-in
[`v2 entity configuration`](https://github.com/proj-airi/game-playing-ai-balatro/blob/9654de554e68ea3367bffbb5574b84563423603d/configs/v2-balatro-entities/dataset.yaml)
points both `train` and `val` at the same image directory. It explicitly marks
that as temporary, so metrics produced with this configuration are not evidence
from an independent holdout.

## What AUV should reuse conceptually

1. **Keep source and consumable projections together.** A canonical typed
   manifest can coexist with generated YOLO files and Hugging Face-compatible
   rows. YOLO should be an export projection, not the source of truth.
2. **Keep full frames.** Full game frames are useful for entity detection,
   context-dependent selection state, overlap, and false-positive negatives.
   Card crops should link back to the immutable parent-frame ID.
3. **Publish an explicit class list and stats.** Class IDs, counts by split,
   negative samples, geometry validation, and discarded-sample reasons should
   be machine-generated artifacts.
4. **Pin datasets in the consumer repository.** The submodule pattern gives the
   model code a reproducible dataset revision, although large generated corpora
   may later prefer an immutable manifest revision over a full checkout.
5. **Retain annotation evidence.** The existing project export is useful for
   auditing manual labels. AUV's equivalent should retain Mod-ground-truth and
   capture-handshake evidence instead of depending on an opaque YOLO TXT.

## Contract AUV should replace it with

The next dataset should separate two tasks that the existing repository blends
under “entities”:

- **Frame entity detection:** broad regions such as card front/back, joker,
  pack, description, stack, tarot, planet, and spectral card.
- **Visible card semantics:** linked card crops/quads with rank, suit, edition,
  enhancement, seal, debuff, facing, and selection supervision masks.

A minimum release layout could be:

```text
dataset.json                         # schema/version/taxonomy/license boundary
splits.json                          # session/run groups, never crop-level random
data/{train,val,test}/metadata.jsonl # one typed frame record per row
frames/<sample_id>.png
crops/<sample_id>/<card_id>.png
exports/yolo/{train,val,test}/...
stats/class-and-domain-coverage.json
provenance/sessions/<session_id>.json
```

Required improvements over the reviewed dataset:

- Use a UUID or content-addressed `sample_id`; preserve original capture name
  only as metadata and reject duplicate IDs before writing.
- Record `session_id`, `run_id`, seed, scenario, parent frame, capture backend,
  frame SHA-256, Balatro/Lovely/Steamodded/Mod versions, language, resolution,
  scale, and active visual Mods.
- Make structured cards/boxes/quads canonical. Generate normalized YOLO lines
  mechanically and verify the projection round trip.
- Split by capture session/run/seed, keeping adjacent frames and all derived
  crops in one split. Provide labeled `val` and `test` partitions.
- Preserve visible-vs-hidden supervision: Mod knowledge must not label an
  invisible rank/suit as a visual target.
- Check class coverage across domains and balance rare effects intentionally;
  do not infer quality from total frame count.
- Fail generation on missing image/label pairs, duplicate IDs, bad class IDs,
  out-of-range geometry, hash changes, or capture-signature mismatches.
- Keep real `auv-driver-linux` frames for final evaluation while optionally
  retaining clean Love framebuffer frames as a separately named source domain.

This makes the Mod-generated corpus a strict superset of the useful parts of
the Project AIRI dataset: it remains exportable to the same YOLO/imagefolder
consumers, while adding exact card supervision, reproducible provenance, and a
real evaluation boundary.
