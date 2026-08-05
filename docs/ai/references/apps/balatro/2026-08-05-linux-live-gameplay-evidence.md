# Balatro macOS-to-Linux live gameplay evidence

Date: 2026-08-05

## Scope and evidence level

This is a live probe, not a support claim. The controller was the AUV CLI and
app plugin on macOS. The selected remote Device was `neko-gpu-1`, where the
daemon, Balatro Runner, screen capture, model inference, and input delivery ran
on Linux/Wayland. The stable Device ID was
`39734f5cd95c64682b0fba06045c37276a5a8c454040a03d4739d7decc356e1e`.

Four blinds were played end to end across two runs. In the first run, the Small
Blind was won after a 296-point straight and a three-ace hand; the Big Blind
reached 448 of the required 450 points and lost by two. After the driver and
action fixes were deployed, a fresh Yellow Deck run won the Small Blind 344/300
and the Big Blind 472/450. The second run used typed blind selection, card play,
discard, cash-out, and store next-round operations throughout. Card submissions
were confirmed by score, hand-count, hands-left, or phase changes rather than
input delivery or image fingerprints alone.

A later routed run added a Business Card Joker and entered the next Boss Blind.
`jokers ls` retained one Driver Runner while it hovered every unread owned
Joker, captured the tooltip frame, and ran OCR. The live result identified
`Business Card` and part of its “Played cards have ... give ... when scored”
effect, changed the reading status to `read`, and cleared `needs_reading`.
Four structurally confirmed hands were then played; the run lost to The Pillar
and reached `game_over`. Most card identities were still null, so the gameplay
policy could only blind-play the first five rank-sorted slots.

Representative durable run evidence:

- Game over after round 2: run `916b0823-f081-1863-9101-e6ddbba0ee8b`, artifact
  `019fcee4-44fe-75c2-81e7-6c76acb6b716.png`.
- First click on New Run changed hover but did not activate: run
  `f54fab01-3e45-818f-fd16-5f14df5ae1d8`, artifact
  `019fceee-4cd3-73b1-9591-75c757a65ea1.png`.
- After adding pointer settle, one click on Play entered the run: run
  `c4c62360-f27d-6d1e-dbc6-3d1be5dbb146`, artifact
  `019fcef0-c034-7b93-938f-df27ff3848d6.png`.
- Fresh-run blind selection: run `18e8c79f-452a-0279-62d2-8c30fb7d2889`,
  artifact `019fcef1-11f2-7691-afdc-f5af5fcd1ead.png`.
- Unobstructed eight-slot hand after clipped-edge filtering: run
  `650bc024-3d83-e0e6-e973-82f11b7dc959`, artifact
  `019fcf00-f506-7923-a2b9-274cb4bbd020.png`.
- Helldivers crash dialog obscuring the live hand: run
  `37f18eb9-c75a-7211-4f8c-d6fe70e828fa`, artifact
  `019fcf00-7e44-76a1-a50f-9ecfa3b92eb6.png`.
- Fresh-run Small Blind at 220/300 before the winning two-pair hand: run
  `b8d7804f-fb6e-4925-9bf1-634674bd6a69`, artifact
  `019fd12d-a9d7-7c13-8d9e-a402682db51e.png`.
- Big Blind full-house result at 412/450: run
  `dd1910a7-3573-ed7a-dacb-a2d03e1e0698`, artifact
  `019fd133-dd88-7b12-bce8-c1e5934585fc.png`.
- Big Blind cash-out at 472/450 with one hand remaining: run
  `db313bcc-521c-aebb-fc19-69a7a573f01b`, artifact
  `019fd136-b23c-7972-9c33-e39605893286.png`.

## Findings and disposition

| Finding | Evidence or root cause | Disposition |
| --- | --- | --- |
| `devices get "$ID"` reported no Device while `devices list` showed the remote Device | Without a root `--device-id`, `get` addressed the local daemon inventory rather than selecting B | Use `auv --device-id "$ID" devices get "$ID"`; CLI discoverability remains a candidate follow-up |
| CUDA Runner crashed with a transport failure | `strace` showed `SIGBUS` loading a 155 MB incomplete uv/rattler temporary `libcublasLt.so.12`; `ldd` later exposed missing cuFFT and cuRAND | Daemon now uses complete stable archive paths for cuDNN, cuBLAS, CUDA runtime, cuFFT, and cuRAND |
| A specialized full-screen card detector produced ten hand slots | A floating 2041x1125 Balatro viewport was being interpreted relative to the 2560x1440 desktop; a narrow duplicate and the deck stack entered the hand | Hand crop now resolves the game viewport from grayscale/polarized edge candidates and maps detections back to display coordinates; regression coverage includes an offset viewport |
| The corrected crop could still expose a ninth `poker_card_back` | The remaining box was a 29x142 deck-stack sliver while complete live cards were approximately 173-197x236-250 | Specialized hand remapping now rejects boxes narrower than 0.30 of their height. The same unobstructed live hand then reported eight slots |
| Play/Discard was sometimes absent even though the hand was playable | UI detector missed the commit button | A typed `PlayingHandLayout` fallback derives a conservative commit point from the two detected Sort Hand controls |
| A delivered click often only established hover | The portal sent absolute motion and button press in the same scheduling slice; Balatro updated hover hit-testing on a later frame | Linux portal input now waits 20 ms after motion, then holds the button for 34 ms. A fresh target subsequently activated with one click |
| `InputActionResult` said delivered when the UI had not changed | Delivery records input transport, not semantic activation; `verified` remained false | Kept the boundary explicit. Game operations perform a separate observation; the generic screen-point command does not claim activation |
| A card play falsely succeeded on hand fingerprint change | Detector jitter and a missed commit control could change fingerprints even when the card remained in hand | Confirmation now requires phase, hand count, observed score, hands-left, or discards-left change. Fingerprint-only evidence becomes `unstable_visual_change` |
| A successful card play was reported as `unstable_visual_change` and Play was clicked twice | The first post-submit capture started during the scoring animation. Inference exceeded the wall-clock settle deadline, so fingerprint evidence triggered a second submit before stable score OCR was observed | Remote card commit now waits before the first observation, permits one additional observation for partial visual evidence, and resubmits only after an explicit `no_hand_state_change`. A regression test and live pair-of-fives play confirmed one submit plus `round_score_changed` |
| Rank/suit output repeated `C_7` across visibly distinct cards | Independent live-domain model misclassification, not association reuse | Unresolved model-quality gap; raw detections remain available and live accuracy must not be inferred from test mAP |
| Non-playing phases exposed a false hand card | On blind-select and cash-out screens, the right-side deck stack was classified as one hand card (for example `H_K` with a negative edition) | Typed phase remained correct, but `hand` is not trustworthy outside `playing`. Phase/zone gating remains intentionally deferred |
| Attribute models labeled the deck stack as glass/foil/red | Full-frame attribute boxes associated with a false hand candidate | Viewport/hand filtering reduces this case, but phase/zone gating remains intentionally deferred |
| Pixel-font OCR missed Play/Discard and some counters | Tesseract did not reliably read Balatro's stylized font; examples included `round_score: "00"` and null hands/discards | Button detection and structural state changes are preferred. OCR replacement or game-state telemetry is a separate slice |
| Plain `game state` output was a Rust debug dump | The app-owned human table has not been designed | JSON is the reliable current interface; table output is deferred at the CLI call site |
| Updating the daemon did not update local orchestration behavior | The macOS `auv-game-balatro` plugin owns action orchestration while the Linux Runner owns observations/input; `cargo check` did not relink the local plugin | Rebuild both the local plugin and affected remote binaries when their respective responsibilities change |
| Typed actions disagreed with an immediately preceding CUDA state command | Actions constructed default CPU configuration with no specialized hand model, producing different slot indices | Remote operation controls now accept `--cards-model` and `--device`; cash-out, restart, next-round, card commit, and blind selection pass that policy through every observation. Argument/config regression coverage is green; final live action replay was interrupted by the external OpenGL failure below |
| A Helldivers crash dialog covered half the hand while Balatro was still visible | Full-display fallback has no typed foreground/occlusion fact; the detector returned only the visible cards | The probe preserved `unknown`/partial evidence instead of inventing covered cards. Foreground and occlusion diagnostics remain a separate contract slice |
| The paired Device became offline although TCP port 19848 remained reachable | The local daemon's paired transport stopped reconnecting after repeated long operations | Restarting only the macOS daemon restored the Device immediately. Reconnect/backoff instrumentation remains a daemon follow-up |
| `input.key alt+f2` was rejected | Linux key parsing omitted standard function-key keysyms | F1-F12 parsing and an Alt+F2 regression test were added. The deployed build then delivered Alt+F4 and closed the exclusive Helldivers window |
| `input.key ctrl+alt+t` reported delivery but the terminal was not initially visible | The shortcut did open Ghostty, but behind Steam; `window.list` could enumerate it while neither `app.activate` nor window-targeted pointer delivery could foreground it | GNOME Overview recovered it manually. Linux window clicks now attempt AT-SPI root focus before portal pointer delivery; app-level activation remains unsupported |
| `input.key super` was rejected | The parser recognized Super only as a shortcut modifier, even though GNOME uses a standalone Super press for Overview | Standalone Ctrl/Shift/Alt/Super parsing and a regression test were added and deployed |
| Screen-point input crashed the runner after daemon restart | `strace` showed the crash immediately after portal motion mapping loaded X11; the service environment had Wayland/DBus variables but no XWayland `DISPLAY` or `XAUTHORITY` | The live daemon was relaunched with `DISPLAY=:0` and the active Mutter Xauthority file. AUV clicks then produced exact X11 ButtonPress/Release coordinates and no crash. A durable service launcher still needs to preserve this environment |
| The new remote runner rejected `cuda:0` after deployment | The isolated Linux build omitted the crate's `cuda` feature; the daemon also retained the already-running non-CUDA runner after the binary was rebuilt | Rebuilt `auv-runner-balatro` with `--features cuda` and restarted the cached runner process. Subsequent typed actions used CUDA successfully |
| Remote `jokers read` and `jokers ls` always returned unread objects | The non-macOS path stopped after entity detection; hover movement and OCR existed only in the local macOS branch | Added the typed `MoveMouse` Driver capability and routed batch hover, capture, and OCR. `jokers ls` now reads pending owned Jokers in one Runner session and returns the Joker list instead of the entire game state. |
| Store Jokers appeared in owned `jokers` | Store products and owned inventory share the `joker_card` detector label | Owned-Joker promotion is now constrained to the upper inventory row; overlapping store class predictions are also deduplicated before slot assignment |
| `store buy --slot store:2` selected different products across observations | Hover tooltips occluded products, and store slot indices are reconstructed from whichever detections survive each frame | Remote buy now clears hover before resolving its frame. Cross-frame store identity is still incomplete when the detector entirely misses an item; callers must not treat a prior transient index as a durable product identity |
| Routed pack read/skip fell back to local macOS resolution | Those command branches have not adopted the inherited AUV context | The accidentally purchased pack was skipped through the generic typed screen-point operation. Routed pack operations remain a separate owner-approved slice |

## Timing and reuse evidence

On the local network, a warm 2560x1440 `display.capture` took approximately
1.9-3.5 seconds. A CUDA `game state` observation took approximately 8.0-9.7
seconds. After the latest settle/confirmation change, verified card commits
typically took about 14-24 seconds. They still make several full observations,
so inference and capture round trips dominate input delivery.

The daemon kept one Linux driver session and reused its portal input and
screencast sessions. The daemon also kept a Balatro Runner process alive. The
Runner caches ONNX pipelines by model configuration; rebuilding the binary does
not replace that live process, which the deployment probe made explicit. This
reuse does not remove the current dominant costs:
seven full-frame preprocessing/inference paths and repeated capture/observation
round trips. Action orchestration now accepts the same `--cards-model` and
`--device` policy as state observation, but it still creates several
short-lived API clients inside one operation.

## Candidate next slices

These are not part of this probe and require owner approval:

- Reuse one remote operation session across the observations made by a typed
  action.
- Add typed foreground/occlusion evidence before treating a partial display
  capture as a complete game viewport.
- Instrument paired-Device reconnect/backoff and recover stale transports
  without restarting the local daemon.
- Phase-gate typed hand inventory while preserving raw detector evidence and a
  typed fallback reason.
- Replace or retrain live-domain card identity recognition and measure per-card
  rank/suit accuracy on real gameplay frames.
- Give store products stable evidence-backed identity across detector misses;
  frame-local vector indices are insufficient for read-then-buy workflows.
- Route pack read/skip/choose through the inherited Device context.
- Add an app-owned human table for `game state`.
- Make Device-selection errors explain the local-inventory versus selected-
  Device distinction.
