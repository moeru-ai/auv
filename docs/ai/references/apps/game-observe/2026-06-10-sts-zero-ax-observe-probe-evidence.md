# Slay the Spire Zero-AX Observe Probe Evidence (M1)

Date: 2026-06-10

Status: core-pipeline evidence for the zero-AX app-family goal (observe-only)

Seam note: `docs/ai/references/recognition/2026-06-10-game-recognition-recipe-consumer-seam.md` (M0)

## What This Is

First live proof that AUV core observe surfaces eat a **zero-AX application
family** (a pure-pixel game) without any core change, plus the honest list of
where the pipeline creaks. This is AUV core evidence. It is **not** an "StS
copilot": no card strategy, no turn planning, no game-state model, no
autonomous play, and no game-specific branch was added anywhere.

Scene arrangement (launching the game, reaching one battle screen, saving and
quitting afterwards) was performed by a human-equivalent operator outside AUV.
Every claim below comes only from recorded AUV runs.

## Setup Facts

- App: Slay the Spire V2.3.4 (12-18-2022), Steam install, launched directly
  from `SlayTheSpire.app`.
- Identity quirk: Info.plist says `com.megacrit.slaythespire`, but the running
  LWJGL/Java process registers as **`net.java.openjdk.cmd`**. All `--target`
  resolution in this evidence uses the runtime id.
- Window: `Slay the Spire`, 1366x796 logical, Retina display scale 2.0.
- Zero core diff: the only repo change in this slice is this note.

## Recorded Runs (all `status: completed`)

Store root: project `.auv/` (gitignored; run ids preserved here for replay
provenance, artifacts live on the recording machine).

| Run | Command | Result |
| --- | --- | --- |
| `run_1781084557891_20225_0` | `debug.probePermissions` | screenRecording/SCK/AX/automation all granted |
| `run_1781084590685_20303_0` | `debug.listWindows` | StS window found (`window_9215`) |
| `run_1781084754378_20407_0` | `debug.captureWindow` | 1366x796 capture + coordinate contract |
| `run_1781084788601_20420_0` | `debug.captureAxTree` | **6 AX nodes, all window chrome; zero content nodes** |
| `run_1781084822814_20445_0` | `debug.observeWindowRegion` (main menu) | RecognitionResult row: `开始游戏 | 设定 | 补丁内容清单 | 退出` |
| `run_1781085049159_20625_0` | `debug.captureWindow` (battle) | full battle frame |
| `run_1781085050164_20629_0` | `debug.observeWindowRegion` (top HUD) | `33铁甲战士 | 2 88/88 | 99` |
| `run_1781085051931_20633_0` | `debug.observeWindowRegion` (energy orb) | `3/3` |
| `run_1781085053571_20637_0` | `debug.findWindowText --query 结束回合` | 1 match, projects to logical `(1237.0, 727.5)` |
| `run_1781085054930_20642_0` | `debug.observeWindowRegion` (enemy band) | `12/12 | 15/15` |
| `run_1781085131686_20682_0` | `debug.observeWindowRegion` (hand cards) | 1 row: `打击` (see limits) |

App probe + analyze (the distill-loop front half, run against the live game):

- `app probe net.java.openjdk.cmd` -> 8/8 steps completed,
  `.auv/app-probes/net-java-openjdk-cmd-1781084703772/probe.json`
- `app analyze` -> `analysis.json` + `report.md`, 4 structured candidates;
  OCR anchors correctly gated `blocked` (missing
  `semantic_verification_contract` + `action_contract`), window-primary-region
  promoted only to `action_grade_candidate` through the v0 window-action seam.

Read-side verified via `auv-cli inspect run_1781085050164_20629_0`: spans
(`auv.command` -> `auv.command.invoke` -> `auv.driver.invoke`), driver events,
and artifact kinds `screenshot`, `window-region-observation`,
`window-region-recognition`, `row-observation.overlay(+annotation)`,
`window-region-segmentation` all present with lineage.

## What This Proves For The Goal

- Zero-AX is real here: content AX surface is empty (6 chrome nodes), so the
  screenshot-first / strongest-available-signal design is doing all the work.
- The intended M2 read commands have observe-side evidence today, through
  existing generic commands only:
  - `sts.readPlayerHp.v0` shape: HUD OCR yields a stable `88/88` numeric form.
  - `sts.readEnergy.v0` shape: energy region OCR yields exactly `3/3`.
  - enemy HP variant: `12/12 | 15/15` from one region observe.
- The intended M3 gated click has grounding evidence (`结束回合` OCR anchor
  with a projected logical point), while promotion gating correctly refuses to
  treat any of it as action-grade without the contracts — the seam held.
- `RecognitionResult` carried every observation (`source: visual_row`,
  `best/filtered/all`, `region_hint`, `capture_artifact` lineage). No second
  recognition schema was needed; the M0 seam decision survives contact with a
  real game.

## Where The Pipeline Creaks (the actual deliverable)

1. **No observe-only strategy taxonomy.** `SkillStrategyTaxonomy::allowed()`
   (`src/skill/mod.rs`) admits 8 combinations, all action-shaped
   (`activation` is mandatory: clipboard/pointer/AX-press variants only), and
   `validate_skill_manifest` enforces the list on every recipe run. An
   observe-only `sts.read*` recipe **cannot validate today**. This blocks M2's
   historical "read commands as recipe-backed commands" direction and needs an owner-approved
   taxonomy extension (e.g. an observe family with a no-op activation and a
   recognition-evidence verification contract). This is the single biggest
   gap between the M0 seam note and an invokable `sts.*` command.
2. **Java app identity is unstable.** Runtime bundle id
   (`net.java.openjdk.cmd`) differs from the bundle's declared id
   (`com.megacrit.slaythespire`), and may differ again under Steam launch.
   Recipes keyed on `app_id` need an honest convention for this family
   (config-level alias, not core special-casing).
3. **`app probe` OCR sample is display-scoped.** The probe's `ocr-sample`
   step OCRs a display capture, so unrelated windows leak into app evidence
   (one recorded anchor was terminal text from the operator's screen:
   `• auv / Slay the Spire AUV core pipeline ~`). Probe sampling should be
   window-scoped for app probes.
4. **Analyzer over-credits chrome AX.** With zero content AX, `app analyze`
   still reports "AX tree contains text-bearing nodes; verifyAxText is a
   viable candidate contract" and recommends a `native-text...verify-ax-text`
   strategy — the only text-bearing node is the window title. The surface
   assessment needs to distinguish window chrome from app content before the
   game family can trust its recommendations.
5. **Row-band OCR cannot enumerate a fanned hand.** The hand region observe
   recognized 1 fragment (`打击`) out of 5 angled, art-heavy cards.
   `sts.listHandCards.v0` is not honestly servable by OCR rows; this is
   exactly where the detector lane (Balatro path, shared `RecognitionResult`
   contract) becomes the stronger signal. No contract change required —
   only a stronger producer.
6. **Synthetic Escape never reached the game.** Two `Escape` presses via
   synthetic events did not open the in-game menu (pointer clicks worked
   throughout). For future M3 verification semantics, keyboard delivery into
   LWJGL windows is unproven; plan on pointer + visual verify.
7. **Retina coordinate readiness.** Probe flags
   `coordinateReadiness=not-ready` (3024x1964 physical vs 1512x982 logical).
   Known probe annotation; must be resolved before any real input slice.

## Boundaries Respected

- No `Candidate` promotion, no clicks, no action delivery by AUV in this
  slice; the End Turn anchor is observe evidence only.
- No `if app == sts` anywhere; no game-specific schema; game specifics exist
  only as run inputs (region ratios, query strings) and this note.
- `candidate_action_*` untouched.

## Next Slice Candidates (observations, not started)

1. Owner-approved design for an observe-only taxonomy combination (unblocks
   M2 `sts.read*` recipes through validator and app-local Rust command paths).
2. `sts.read*` recipe and case matrix once (1) lands, reusing
   `debug.observeWindowRegion` and signal expects on the numeric formats.
3. Window-scoped probe OCR sampling and chrome-vs-content AX distinction
   (fixes creaks 3 and 4 for every future app family, not just games).
