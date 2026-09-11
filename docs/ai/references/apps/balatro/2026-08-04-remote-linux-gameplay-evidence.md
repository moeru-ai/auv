# Balatro remote Linux gameplay evidence

Date: 2026-08-04

Status: live behavior evidence for one paired Debian Device running Balatro
through Steam/Proton. This raises the Balatro extension above compilation-only
evidence for the exercised operations; it does not claim general Linux or
cross-game support.

## Environment

- caller: macOS root `auv` CLI;
- Device: paired `neko-gpu-1` Debian host;
- game: Steam app `2379780`, Proton/LÖVE, Chinese UI;
- capture: `xdg-desktop-portal.screencast.pipewire`, 2560×1440 primary display;
- core RunnerClass: `auv.core.local`;
- app RunnerClass: `auv.game.balatro`;
- app provider: `supported/games/auv-game-balatro/runner-provider.example.json`.

The Proton window was not available through AT-SPI, so observation used the
explicit primary-display fallback. This was not a screenshot-portal permission
failure.

The transient daemon service included the Wayland, X11, user D-Bus, and runtime
directory environment. Debian's Chinese Tesseract model was downloaded and
extracted without root under `~/.local/share/auv`; a combined user tessdata
directory exposes both the existing English model and downloaded Chinese model.

## Live commands and outcomes

All commands used the root surface:

```text
auv --device-id <device-id> game-balatro ...
```

Observed evidence:

1. `game state --json` routed through the inherited context, captured the
   remote display, resolved both detector models on the Runner host, and
   classified `main_menu`.
2. `game restart --verify --details` delivered coordinate-qualified screen
   clicks and eventually verified `blind_select`.
3. `blinds select --slot blind:0 --verify --details` showed the first delivered
   click did not start play and the second command verified `playing` with
   eight hand cards.
4. `cards play --slots hand:0,hand:1,hand:2,hand:3,hand:4 --verify` selected five
   typed hand slots, detected the Play control, retained every Driver delivery
   result, and verified the commit against a post-selection baseline.
5. Three games reached a stable `game_over` state with detected
   `button_new_run` and `button_main_menu`. Games two and three each ended after
   three verified hand refreshes and a fourth commit verified by both phase and
   hand-count changes. Repeated game-over restart reached `blind_select` between
   games, proving this was not a one-shot title-screen path.
6. A later traced run exercised cash-out, store next-round, blind selection,
   and another four-hand game. The fourth hand verified a phase and hand-count
   change and the following observation classified stable `game_over`.

Direct core capture evidence includes run
`839ec1e2-5dd8-4beb-9bf7-25cf44ed6512`, which recorded a portal PNG artifact
and the 2560×1440 screenshot-to-logical bounds contract.

## Bugs reproduced and fixed

### Large desktop frames

Tonic's default 4 MiB decoding limit rejected a 2560×1440 RGBA frame. Core
Capture and OCR clients now share an explicit image RPC message policy. A
regression fixture returns a frame larger than 4 MiB.

### Caller-host model paths

The detector request previously sent a path resolved on the macOS caller to the
Linux Runner. The protocol now distinguishes a Runner path from a Hugging Face
asset, and the Runner resolves repository assets on its own host.

### Nested async runtime

Blocking Hugging Face class-asset loading entered its runtime from a Tokio
thread. Class loading now runs in `spawn_blocking`; a regression test loads
local class assets outside the async runtime thread.

### False card submission verification

The original commit confirmation compared the after frame with the
pre-selection frame. Merely raising cards changed fingerprints and falsely
confirmed Play. The baseline is now the fully selected state, and fingerprint
change only proves submission when Play/Discard controls also clear. The remote
path retries one delivered-but-unconfirmed submit click. A root-cause regression
test covers selection-only fingerprint change.

### Overlay/background control ambiguity

The New Run modal is translucent, so the detector can see the title button
underneath it. Restart now waits for expected transitions and uses a documented
modal layout step where the current dataset cannot reliably distinguish tabs.
The layout deferral is marked at the call site with the dataset trigger.

### Focus-only first clicks

Linux foreground delivery can report a successful system-event click while the
first click only focuses the Proton window. Blind selection reproduced this in
one command: attempt one left `blind_select` visible, attempt two reached
`playing` with eight hand cards. Blind selection, cash-out, and store
next-round now retain typed attempt lists and retry only after semantic
verification remains unconfirmed.

### Transient post-submit frames

The first frame after Play can still show the selected hand even though the
submission is in flight. Card commit now polls bounded post-click observations
against the selected-state baseline before deciding whether another delivery
is necessary. A traced live commit retained two submit attempts and ultimately
confirmed changed hand fingerprints.

### Payout phase and detector promotion

`button_cash_out` was detected at high confidence but the frame was classified
as `unknown`. `cash_out` is now an explicit app-owned phase. A low-confidence
`tarot_card` false positive on the right deck stack was also being promoted to
owned consumable inventory; promotion is now restricted to the persistent
top-right consumable band while raw detector evidence remains inspectable.

### Numeric glyph false positives

The scaled score glyph `5` was read as `3`, and an ambiguous `295` round score
was published as `333`. The thick Balatro five has a regression template, and
multi-digit reads now fail closed when any glyph is ambiguous or insufficiently
separated from the runner-up. The fixed frame now reports chips `5` and leaves
the unresolved round score absent instead of publishing a false value.

## Tracing and fixed-frame diagnostics

Run `22cb0694-0d7b-69d6-086d-ac03d6bd7aa7` contains, in order:

- `auv.frontend.lifecycle` for the Balatro plugin;
- repeated `auv.balatro.observation.capture` PNG artifacts;
- matching `auv.balatro.observation.state` JSON artifacts;
- one `auv.driver.input_action_result` artifact per delivered click; and
- `auv.balatro.card_commit.completed` with requested slots, all selection and
  submit actions, and the semantic confirmation.

The `diagnostics` command accepts a fixed `--image`, expected
`--requested-slots`, and explicit annotated/report output paths. On the final
hand fixture it proved all requested slots `hand:0..4` were raised, all eight
computed click points were inside their own boxes, and selection matched the
request. The visual content was `A♠ Q♥ 7♣ 6♠ 5♣`; suit extraction now reports
`♠ ♥ ♣ ♠ ♣`. Rank-template candidates remain diagnostic-only because the
current fixed-frame evidence does not justify promoting them to verified card
content.

## Evidence level and remaining gaps

| Surface | Evidence |
|---|---|
| Remote core display capture | Live behavior plus PNG artifact |
| Remote app recognition Runner | Live behavior across repeated frames |
| Restart to blind selection | Live semantic verification |
| Blind selection | Live semantic verification |
| Five-card submission | Live delivery plus strengthened semantic verification |
| Repeated complete games | Three stable game-over outcomes |
| Durable remote plugin tracing | Live events, PNG/state artifacts, and typed input artifacts in one Run |

The root CLI now passes the frontend-owned tracing store root separately from
`AUV_CONTEXT`; the plugin installs a file-backed dispatch for the inherited Run
and flushes before exit. This closes the earlier evidence gap without putting
filesystem policy into the routed client context.
