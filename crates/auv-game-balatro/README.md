# auv-game-balatro Live Probe Handoff

Date: 2026-06-08

This README records the live Balatro operations that were first validated with
short Python probes in `proj-airi/game-playing-ai-balatro`, then should be
migrated into reusable `auv-game-balatro` Rust CLI operations.

The Python probes were temporary evidence-gathering tools. They should not
become the long-term automation surface. The Rust migration should keep the
current AUV seam:

```text
recognition / candidates
  -> target resolution
  -> auv-driver input
  -> operation result / verification evidence / artifacts
```

`auv-overlay-macos` is not an input backend for this crate.

## Current Rust Surface

`auv-game-balatro` already has the object-oriented CLI shape:

```bash
auv-game-balatro game state
auv-game-balatro game restart --verify
auv-game-balatro game cash-out --verify
auv-game-balatro cards ls
auv-game-balatro cards read --slot hand:all
auv-game-balatro cards clear --verify
auv-game-balatro cards select --slots hand:0,hand:2
auv-game-balatro cards play --slots hand:0,hand:2 --verify
auv-game-balatro cards discard --slots hand:1,hand:3 --verify
auv-game-balatro store status
auv-game-balatro store ls
auv-game-balatro store read --slot store:0
auv-game-balatro store buy --slot store:0 --verify
auv-game-balatro store next-round --verify
auv-game-balatro consumables read --slot consumable:0
auv-game-balatro consumables use --slot consumable:0 --verify
auv-game-balatro jokers read --slot joker:0
auv-game-balatro pack read --json
auv-game-balatro pack choose --slot pack:1 --verify
auv-game-balatro pack skip --verify
auv-game-balatro blinds ls
auv-game-balatro blinds select --slot blind:0 --verify
auv-game-balatro blinds skip --verify
```

`LOG.md` records commands, stdout/stderr, parsed JSON, screenshots copied from
operation outputs, and the natural-language decision for each step. `SKILL.md`
records the current reusable play policy. The current policy is intentionally
conservative: buy early jokers first, skip unread packs until hover OCR is
available, play flushes when confidently read, play rank groups when present,
and use up to two weak-hand discards per blind before falling back to high-card
play.

The remaining deferred mutating operations are:

```bash
auv-game-balatro store reroll --verify
auv-game-balatro jokers sell --slot joker:0 --verify
```

Default Balatro observation uses Hugging Face cached assets instead of an
owner-local checkout. The entities and UI ONNX models are resolved from
`proj-airi/games-balatro-2024-yolo-entities-detection` and
`proj-airi/games-balatro-2024-yolo-ui-detection`; class lists are resolved from
the matching `proj-airi/games-balatro-2024-entities-detection` and
`proj-airi/games-balatro-2024-ui-detection` dataset repositories. The optional
card-corner ONNX classifier resolves from
`proj-airi/games-balatro-2024-card-corner-classifier` when the
`card-corner-onnx` feature loads that classifier. Explicit `--entities-model`,
`--entities-classes`, `--ui-model`, `--ui-classes`, and `--card-corner-model`
arguments remain the highest-priority override.

`cards read` can optionally use the local Balatro deck atlas as a visual
template fallback for rank glyphs. AUV does not redistribute this game asset.
Run setup on a machine with a valid local Steam install to extract the atlas
into AUV's local cache:

```bash
auv-game-balatro setup
auv-game-balatro setup --check
```

If Balatro is installed outside the default Steam location, pass
`--app <Balatro.app>` or `--love <Balatro.love>`. Runtime atlas lookup reads the
setup cache only; there is no owner-local fallback path and no environment
variable override.

This slice implements the P0 store and pack operations except `store reroll`
and `jokers sell`, which remain deferred mutating commands. `store read`,
`jokers read`, `consumables read`, and `pack read` now use live hover OCR when
they are pointed at the running game. Image-backed reads still report static
observation evidence because they cannot move the pointer or create a tooltip.
The current read path keeps raw OCR text and evidence artifacts first; structured
name/effect parsing remains a read-quality follow-up.

Pack operations are now a top-level namespace because an opened card pack is an
active modal-like game surface, not simply a store row item. Current active
pack choice support is intentionally limited to joker, tarot, planet, and
spectral choices. Standard Pack playing-card choices are deferred because
`poker_card_front` also appears on persistent hand/deck cards and needs a
separate live fixture before it is safe to target.

## Live Probe Commands

The probes used `ScreenCapture`, `MultiYOLODetector`, `MouseController`, and
`CardTooltipService` from the Python repository:

```bash
cd <game-playing-ai-balatro>
PYTHONPATH=src pixi run python
```

The local Balatro window was detected as a visible display-region capture:

```json
{
  "top": -1094,
  "left": -751,
  "width": 1646,
  "height": 963
}
```

The negative coordinates are expected with the current multi-display layout.
Click delivery still worked when frame coordinates were projected through the
capture region.

### Arcana Pack Hover OCR

The probe captured the open Arcana Pack, selected lower-row `tarot_card`
detections, hovered each card, ran YOLO again to find `card_description`, and
OCRed the description crops.

Representative command:

```bash
PYTHONPATH=src pixi run python - <<'PY'
from pathlib import Path
import cv2
from ai_balatro.core.screen_capture import ScreenCapture
from ai_balatro.core.multi_yolo_detector import MultiYOLODetector
from ai_balatro.ai.actions.mouse_controller import MouseController
from ai_balatro.services.card_tooltip_service import CardTooltipService

out = Path(".runs/balatro-arcana-pack-hover-choices")
out.mkdir(parents=True, exist_ok=True)

screen = ScreenCapture()
mouse = MouseController(screen)
multi = MultiYOLODetector()
tooltip = CardTooltipService(
    screen_capture=screen,
    mouse_controller=mouse,
    multi_detector=multi,
)

frame = screen.capture_once()
entities, ui = multi.detect_combined(frame, confidence_threshold=0.2)
tarots = sorted(
    [
        detection
        for detection in entities
        if detection.class_name == "tarot_card"
        and detection.center[1] > frame.shape[0] * 0.48
    ],
    key=lambda detection: detection.center[0],
)[:5]

for index, card in enumerate(tarots):
    screen_x, screen_y = tooltip._frame_to_screen_coordinates(
        card.center[0],
        card.center[1] - 20,
        frame.shape,
    )
    mouse.smooth_move_to(screen_x, screen_y)
    hover_frame = screen.capture_once()
    cv2.imwrite(str(out / f"hover-{index:02d}.png"), hover_frame)
    hover_entities, hover_ui = multi.detect_combined(
        hover_frame,
        confidence_threshold=0.2,
    )
    for description in [
        detection
        for detection in hover_entities + hover_ui
        if detection.class_name in {"card_description", "poker_card_description"}
    ]:
        x1, y1, x2, y2 = map(int, description.bbox)
        crop = hover_frame[y1:y2, x1:x2]
        print(index, tooltip._ocr_description_crop(crop))
PY
```

Observed OCR output:

```text
正义 - 增强1张选定卡牌成为玻璃牌
命运之轮 - 有1/4几率给一张随机小丑牌添加版本
恋人 - 增强1张选定卡牌成为万能牌
皇后 - 增强2张选定卡牌成为倍率牌
战车 - 增强1张选定卡牌成为钢铁牌
```

Artifacts:

```text
.runs/balatro-arcana-pack-hover-choices/hover-scan.json
.runs/balatro-arcana-pack-hover-choices/initial-detections.png
.runs/balatro-arcana-pack-hover-choices/hover-00-detections.png
.runs/balatro-arcana-pack-hover-choices/hover-01-detections.png
.runs/balatro-arcana-pack-hover-choices/hover-02-detections.png
.runs/balatro-arcana-pack-hover-choices/hover-03-detections.png
.runs/balatro-arcana-pack-hover-choices/hover-04-detections.png
```

### Arcana Pack Choose

The probe selected Wheel of Fortune, then clicked a layout fallback confirm
point below the selected card. After confirmation, `button_card_pack_skip`
disappeared and the view returned to store context.

Representative command:

```bash
PYTHONPATH=src pixi run python - <<'PY'
from pathlib import Path
import time
from ai_balatro.core.screen_capture import ScreenCapture
from ai_balatro.core.multi_yolo_detector import MultiYOLODetector
from ai_balatro.ai.actions.mouse_controller import MouseController

out = Path(".runs/balatro-arcana-wheel-choice-trial")
out.mkdir(parents=True, exist_ok=True)

screen = ScreenCapture()
mouse = MouseController(screen)
multi = MultiYOLODetector()

frame = screen.capture_once()
entities, ui = multi.detect_combined(frame, confidence_threshold=0.2)
tarots = sorted(
    [d for d in entities if d.class_name == "tarot_card" and d.center[1] > frame.shape[0] * 0.48],
    key=lambda d: d.center[0],
)[:5]
wheel = tarots[1]
region = screen.get_capture_region()

mouse.click_at(region["left"] + wheel.center[0], region["top"] + wheel.center[1])
time.sleep(1.0)

fallback_x = region["left"] + wheel.center[0]
fallback_y = region["top"] + int(frame.shape[0] * 0.82)
mouse.click_at(fallback_x, fallback_y)
time.sleep(1.4)
PY
```

Artifacts:

```text
.runs/balatro-arcana-wheel-choice-trial/summary.json
.runs/balatro-arcana-wheel-choice-trial/after-click-card-detections.png
.runs/balatro-arcana-wheel-choice-trial/after-confirm-fallback-detections.png
```

Design note: pack choice confirmation needs both a YOLO `button_use` path and a
recorded layout fallback. The fallback must report the clicked frame point,
screen point, and post-click detections.

### Store Item Hover and Buy

The probe read store products by hovering card-like detections in the shop row,
then selected the left joker and clicked the detected `button_purchase`.

Representative command:

```bash
PYTHONPATH=src pixi run python - <<'PY'
from ai_balatro.core.screen_capture import ScreenCapture
from ai_balatro.core.multi_yolo_detector import MultiYOLODetector
from ai_balatro.ai.actions.mouse_controller import MouseController

screen = ScreenCapture()
mouse = MouseController(screen)
multi = MultiYOLODetector()
region = screen.get_capture_region()

frame = screen.capture_once()
entities, ui = multi.detect_combined(frame, confidence_threshold=0.15)
height, width = frame.shape[:2]
products = sorted(
    [
        detection
        for detection in entities
        if detection.class_name in {"joker_card", "planet_card", "tarot_card", "spectral_card", "card_pack"}
        and height * 0.25 < detection.center[1] < height * 0.70
        and width * 0.35 < detection.center[0] < width * 0.80
    ],
    key=lambda detection: detection.center[0],
)

target = next((d for d in products if d.class_name == "joker_card"), products[0])
mouse.click_at(region["left"] + target.center[0], region["top"] + target.center[1])

selected = screen.capture_once()
selected_entities, selected_ui = multi.detect_combined(selected, confidence_threshold=0.15)
purchase = max(
    [d for d in selected_ui if d.class_name == "button_purchase"],
    key=lambda d: d.confidence,
)
mouse.click_at(region["left"] + purchase.center[0], region["top"] + purchase.center[1])
PY
```

Observed OCR:

```text
store:0 joker_card - 致胜之拳
store:1 planet_card - 木星, 等级1, 升级同花, +2倍率, +15筹码
```

Artifacts:

```text
.runs/balatro-store-item-buy-trial/summary.json
.runs/balatro-store-item-buy-trial/hover-00-detections.png
.runs/balatro-store-item-buy-trial/hover-01-detections.png
.runs/balatro-store-item-buy-trial/after-purchase-detections.png
```

Observed verification:

```text
cash: $9 -> $4
joker slots: 1/5 -> 2/5
```

### Planet Buy and Use

The probe bought Jupiter from the store, then clicked the held consumable and
clicked `button_use`.

Representative command:

```bash
PYTHONPATH=src pixi run python - <<'PY'
from ai_balatro.core.screen_capture import ScreenCapture
from ai_balatro.core.multi_yolo_detector import MultiYOLODetector
from ai_balatro.ai.actions.mouse_controller import MouseController

screen = ScreenCapture()
mouse = MouseController(screen)
multi = MultiYOLODetector()
region = screen.get_capture_region()

frame = screen.capture_once()
entities, ui = multi.detect_combined(frame, confidence_threshold=0.15)
planet = max(
    [d for d in entities if d.class_name == "planet_card" and 300 < d.center[1] < 650],
    key=lambda d: d.confidence,
)
mouse.click_at(region["left"] + planet.center[0], region["top"] + planet.center[1])

selected = screen.capture_once()
_, selected_ui = multi.detect_combined(selected, confidence_threshold=0.15)
purchase = max([d for d in selected_ui if d.class_name == "button_purchase"], key=lambda d: d.confidence)
mouse.click_at(region["left"] + purchase.center[0], region["top"] + purchase.center[1])

held = screen.capture_once()
held_entities, _ = multi.detect_combined(held, confidence_threshold=0.15)
held_planet = max(
    [d for d in held_entities if d.class_name == "planet_card" and d.center[0] > held.shape[1] * 0.65],
    key=lambda d: d.confidence,
)
mouse.click_at(region["left"] + held_planet.center[0], region["top"] + held_planet.center[1])

selected = screen.capture_once()
_, selected_ui = multi.detect_combined(selected, confidence_threshold=0.15)
use = max([d for d in selected_ui if d.class_name == "button_use"], key=lambda d: d.confidence)
mouse.click_at(region["left"] + use.center[0], region["top"] + use.center[1])
PY
```

Artifacts:

```text
.runs/balatro-buy-jupiter-trial/summary.json
.runs/balatro-buy-jupiter-trial/after-purchase-detections.png
.runs/balatro-use-jupiter-trial/summary.json
.runs/balatro-use-jupiter-trial/after-use-detections.png
```

Observed verification:

```text
cash: $4 -> $1
held consumables: 0/2 -> 1/2 -> 0/2 after use
flush display: 同花 等级2
score parameters after use: 50 chips, 6 mult
```

### Store Next Round Fallback and Blind Select

The store next-round button was visible but YOLO missed
`button_store_next_round` in one capture. A layout fallback clicked the known
shop-panel button center, then verification observed blind-select controls.

Representative command:

```bash
PYTHONPATH=src pixi run python - <<'PY'
from ai_balatro.core.screen_capture import ScreenCapture
from ai_balatro.core.multi_yolo_detector import MultiYOLODetector
from ai_balatro.ai.actions.mouse_controller import MouseController

screen = ScreenCapture()
mouse = MouseController(screen)
multi = MultiYOLODetector()
region = screen.get_capture_region()

# Layout fallback from the stable shop panel location.
frame_x, frame_y = 594, 429
mouse.click_at(region["left"] + frame_x, region["top"] + frame_y)

after = screen.capture_once()
entities, ui = multi.detect_combined(after, confidence_threshold=0.15)
select = max([d for d in ui if d.class_name == "button_level_select"], key=lambda d: d.confidence)
mouse.click_at(region["left"] + select.center[0], region["top"] + select.center[1])
PY
```

Artifacts:

```text
.runs/balatro-next-round-layout-fallback-trial/summary.json
.runs/balatro-next-round-layout-fallback-trial/after-click-detections.png
.runs/balatro-select-next-blind-trial/summary.json
.runs/balatro-select-next-blind-trial/after-select-detections.png
```

Observed verification:

```text
store -> blind_select -> playing
button_level_select detected after next-round fallback
joker cards detected after blind select: 4
joker counter: 4/5
```

The last observation verified that the Riff-raff-like joker selected from the
Buffoon Pack generated two common jokers when the next blind was selected.

## Reusable Operation Lessons

### Capture and Coordinates

- Balatro can require display-region capture when window capture times out.
- Capture frame coordinates must be projected through the current capture
  region or window frame before input.
- If the window moves or resizes between observation and action, old bboxes are
  unsafe. Live operations should capture immediately before resolving targets.

### Store Items

- Store item detection should not use all `joker_card` / `planet_card`
  detections blindly.
- The top owned-joker row and right deck stack must be excluded.
- Price detections and known shop-panel bands are useful evidence for store item
  slots.
- `store buy` should select the item first, then find `button_purchase` or
  `button_use` in a fresh post-selection capture.

### Packs

- Packs and pack choices need hover OCR.
- A selected pack choice may expose `button_use`, but the visible green confirm
  button is not always stable in the UI model.
- Pack choice operations need a fallback confirm target, with fallback reason
  and post-click evidence in the result.

### Consumables

- Held tarot / planet / spectral cards should be modeled as `consumable:N`.
- `consumables use` should click the held card, recapture, find `button_use`,
  then click it.
- Verification can start with state evidence and screenshots; semantic effects
  such as `同花 等级2` should be added as OCR-backed evidence later.

### Hover OCR

- Joker, tarot, planet, pack, voucher, and uncertain poker cards should be
  readable through hover plus OCR. Live joker, consumable, store-item, and pack
  choice reads now use this path; static image reads still expose the unresolved
  observation instead of inventing tooltip text.
- The Rust slice uses existing AUV/macOS OCR and local recognizers only. It
  must not guess owner-local Python repositories, import Python OCR packages,
  or keep ad-hoc external OCR command hooks.
- TODO(balatro-first-party-ocr): evaluate a first-party OCR tool or library
  integration when hover text/card text quality becomes the approved slice.
- The operation result should include raw text, OCR confidence when available,
  tooltip bbox, object bbox, hover point, and artifact paths.

### TODO: Live Hover Read Quality

- Separate owned joker slots from shop joker-like detections in store phase.
  Live `jokers read` can currently pick visible shop objects if they share the
  same detection class and overlap the broad object filter.
- Serialize hover-based read commands or add an explicit pointer/session guard.
  Concurrent `store read`, `jokers read`, `consumables read`, or `pack read`
  calls fight over the mouse and can produce empty detector results.
- Normalize raw hover OCR into structured `name`, `edition`, `rarity`, `cost`,
  and `effect` fields while still preserving raw OCR text and crop artifacts.
- Tune per-object tooltip crop regions for owned jokers, held tarot/planet/
  spectral consumables, store items, vouchers, and active pack choices.
- Add live fixture or artifact-backed regression coverage for tarot, planet,
  spectral, joker, store-item, pack-choice, and voucher reads.
- Add voucher-specific read and purchase grounding. Vouchers matter
  strategically, but their detection and buy target should not be folded into
  generic store-card handling without evidence.
- Keep score/current-round digit reading as a separate OCR-quality slice.
  Recent live runs still showed score misreads, and that path should not be
  conflated with tooltip OCR.

### TODO: DebugPlus Ground Truth Adapter

- Investigate the DebugPlus mod as a development-only ground-truth channel for
  Balatro observations.
- Add a tiny AUV DebugPlus command such as `auv_dump_state` that writes JSON
  through `love.filesystem.write(...)` instead of changing gameplay state.
- The dump should include current phase, dollars, hands/discards left, ante,
  round, hand cards, jokers, consumables, shop items, pack choices, and each
  card object's internal rank/suit/center/ability plus `T.x/T.y/T.w/T.h` when
  available.
- Use this only for benchmarks, labeled screenshots, OCR/CNN error-rate
  reports, and coordinate-grounding investigations. It must not become the
  normal CLI gameplay path, because AUV still needs to prove the unmodded
  visual/input contract works.
- Compare the DebugPlus dump against AUV observation output after every
  screenshot to produce structured false-positive, false-negative, and
  rank/suit mismatch reports.

### Verification

- Click success is not semantic success.
- For cards, hand count is weak because Balatro refills the hand after play.
  Current Rust verification accepts phase changes, hand-count changes, or
  before/after hand fingerprint changes, and reports the evidence used.
- `--no-cache` bypasses persisted card-reading reuse, but still keeps
  current-frame visual fingerprints so action verification can prove state
  changes.
- For store buy, useful checks are cash delta, store item disappearance,
  inventory count changes, and after-image evidence. Current Rust verification
  reports store item, joker, consumable, and phase-change evidence.
- For next-round and blind-select, phase transition and button-set changes are
  useful checks.
- Failed verification should return structured evidence instead of hiding the
  action result.

## Migration Target

The next Rust slice should keep changes inside:

```text
crates/auv-game-balatro/src/cli.rs
crates/auv-game-balatro/src/model.rs
crates/auv-game-balatro/src/observation.rs
crates/auv-game-balatro/tests/
```

Possible narrow order:

1. Add shared object-slot parsing and selection helpers for `store:N`,
   `joker:N`, `consumable:N`, and `pack:N`.
2. Add `store buy --slot store:N --verify`.
3. Add `consumables use --slot consumable:N --verify`.
4. Harden live hover reads with owned/store zone separation, serialized pointer
   use, and structured raw-text normalization.
5. Add voucher-specific read/buy grounding and regression fixtures for
   tarot/planet/spectral/joker/store/pack object reads.

Do not broaden this into Balatro strategy, RL environment replication, or root
AUV runtime/catalog integration in this slice.

## Proposed Reusable Commands

The command surface should stay object-oriented. The CLI resolves visible game
objects and records evidence; an external agent decides whether an action is a
good strategy.

One naming rule is useful for packs:

- Store packs remain `store:N` items before purchase.
- The active opened pack screen can use `pack:N` choice slots after opening.

| Command | Required observation | Action target | Verification | Evidence |
| --- | --- | --- | --- | --- |
| `store status` | Store controls and store item zones | None | Observation only | Frame, store summary, raw detections |
| `store ls` | Store item detections; hover OCR when unread/stale | None | Observation only | Per-item bbox, hover image, OCR text, cache hint |
| `store buy --slot store:N` | Store state, target item bbox, confirm button after selection | Store item, then `button_purchase` or `button_use` | Item disappears, cash changes, or item appears in held zone | Before/selected/after frames, target bbox, confirm button, verification evidence |
| `store reroll` | `button_store_reroll`, item fingerprints | Reroll button | Store item fingerprints change | Before/after item list, button bbox |
| `store next-round` | Prefer `button_store_next_round`; layout fallback if missed | Next-round button or fallback point | Store controls disappear; phase changes | Target source, fallback reason, after frame |
| `store open-pack --slot store:N` | `store:N` is `card_pack`; pack hover OCR if unread | Buy/open target | Pack choices or skip button visible | Pack OCR, before/after frames |
| `pack read` | Active non-Standard Pack choices, skip button, optional confirm/use button | None | Observation only | Choice bboxes, hover screenshots, OCR crops/text when live |
| `pack choose --slot pack:N` | Active non-Standard Pack choice slot | Choice card, then confirm/use button or fallback point | Pack screen closes or selected item appears | Before/selected/after frames, confirm target source |
| `pack skip` | `button_card_pack_skip` | Skip button | Pack screen closes | Skip button bbox, after frame |
| `consumables ls` | Held tarot/planet/spectral zones | None | Observation only | Slot bboxes, hover OCR text, cache hint |
| `consumables read --slot consumable:N` | Held consumable bbox | None | Reading exists or explicit uncertainty | Hover screenshot, OCR crop, text/source |
| `consumables use --slot consumable:N` | Held consumable bbox, `button_use` after selection | Consumable, then use button or fallback point | Consumable leaves held zone or visible effect changes | Before/selected/after frames |
| `jokers ls` | Joker zone detections | None | Observation only | Joker bboxes, hover OCR text, cache hint |
| `jokers read --slot joker:N` | Joker bbox | None | Reading exists or explicit uncertainty | Hover screenshot, OCR crop, text/source |
| `jokers sell --slot joker:N` | Joker bbox, sell affordance after selection | Joker, then `button_sell` | Joker removed; cash changes if readable | Before/after joker list, sell button bbox |
| `cards read --slot hand:N` | Fresh hand detections; corner OCR/template evidence | None | Card identity is known/partial/uncertain | Frame, bbox, crop, OCR/template evidence |
| `cards clear --verify` | Fresh hand slots and selected-state capture | Currently selected hand slots only | No selected hand slots remain | Before/after frames, per-card interaction state |
| `cards select --slots ...` | Fresh hand slots and selected-state capture | Requested card slots, with observe-gated retry | Selected hand slot set exactly matches requested slots | Before/selected frames, clicked points, per-card interaction state |
| `cards play --slots ...` | Fresh hand slots, exact selected-state gate, play button | Clear non-requested selected slots, select missing requested slots, then `button_play` | Exact selected-state gate passes before commit; phase, hand count, or hand fingerprint changes after commit | Before/selected/after frames, verification evidence |
| `cards discard --slots ...` | Fresh hand slots, exact selected-state gate, discard button | Clear non-requested selected slots, select missing requested slots, then `button_discard` | Exact selected-state gate passes before commit; phase, hand count, or hand fingerprint changes after commit | Before/selected/after frames, verification evidence |
| `game restart` | New Run play button, or Game Over layout fallback | `button_new_run_play` or fallback point | Blind-select, playing, or store phase appears | Before/intermediate/after frames, strategy |
| `game cash-out` | `button_cash_out` | Cash-out button | Cash-out button disappears or store appears | Before/after frames, button bbox |

Shared mutation result shape:

```text
operation
target_object
observation_before
action_target
driver_result
verification
artifacts
fallbacks
```

Important fallback contracts:

- `store next-round`: try `button_store_next_round`; if YOLO misses the visible
  button, use deterministic store-panel layout fallback and record
  `fallback_reason = "yolo_button_missing_visible_layout_match"`.
- `pack choose`: try detected confirm/use button; if absent, use the green
  confirm/use layout fallback and record the fallback target.
- `pack read` / `pack choose`: current choice detection excludes Standard Pack
  `poker_card_front` choices until that label can be separated from hand/deck
  playing-card detections with live fixture evidence.
- Hover OCR is required for jokers, tarot/planet/spectral consumables, store
  items, pack choices, vouchers, and uncertain poker cards. The live path now
  exists for the main object reads; remaining work is quality, structure,
  serialization, and coverage.
- Card actions must rebuild slot identity from a fresh observation before every
  play/discard. Live runs showed slot assumptions drift after discards.

## Smallest Next Slices

The next work should not try to implement every command in the table at once.
The current crate already has card play/discard, store next-round, blind
select/skip, live capture fallback, macOS Vision card-corner OCR, local UI
digit readers, and deck template rank inference.

Recommended order:

1. Add tests and helpers for `store:N`, `joker:N`, `consumable:N`, and `pack:N`
   slot parsing and object selection.
2. Harden live object reads by separating owned zones from shop zones, adding
   hover-session serialization, and normalizing raw OCR into structured fields.
3. Add voucher read/buy grounding with evidence distinct from generic store
   cards.
4. Add live fixture or artifact-backed tests for joker, tarot, planet,
   spectral, store-item, pack-choice, and voucher reads.
5. Improve score/current-round OCR separately from tooltip OCR.
