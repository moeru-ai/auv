# QQ Music macOS Capability Probe

Date: 2026-05-15

Status: capability probe report

## Purpose

This report captures the current capability boundary for automating QQ Music on
macOS through AUV primitives.

It exists to stop the project from making false claims such as:

- QQ Music is fully semantic-controllable through AX
- QQ Music can already be automated without user disturbance
- the current search recipe is equivalent to a playback skill

The goal here is narrower and more useful:

- record what the current app surface actually looks like
- record which control paths are proven, weak, or disproven
- set the next implementation gates for AUV primitives and recipes

## Environment

- macOS: `26.4.1 (25E253)`
- QQ Music: `11.1.1 (73262)`
- bundle id: `com.tencent.QQMusicMac`
- main executable: `/Applications/QQMusic.app/Contents/MacOS/QQMusic`
- sampled pid during the probe: `19024`
- accessibility permission: granted
- screen recording permission: granted
- automation permission: not explicitly re-verified in this probe

## App Surface

The current QQ Music build should be treated as a native macOS shell with an
embedded WebKit content surface.

Observed facts:

- the app links `WebKit` and `JavaScriptCore`
- bundle strings include `WKWebView`, `WebView`, and `QMWKWebView`
- no `Electron Framework`, `Chromium Embedded Framework`, or `Qt` surface was
  found in the sampled inspection
- no QQ Music-owned browser helper process was visible in the sampled process
  list

This matters because the app is not a normal browser target. The result surface
behaves like a black-box WebView instead of a stable AX-first or CDP-first UI.

## CDP / Browser Surface

The sampled QQ Music process listened on multiple localhost ports:

- `127.0.0.1:60425`
- `127.0.0.1:60426`
- `127.0.0.1:60437`
- `127.0.0.1:60438`

However, `curl http://127.0.0.1:<port>/json/version` returned `404 Not found!`
for all of them.

Current conclusion:

- no standard CDP surface is proven
- QQ Music should not currently be treated as browser-automatable
- deeper private-port reverse engineering is not justified for the current AUV
  milestone

## AX Tree Findings

The AX tree is partially useful, but not sufficient for the whole workflow.

What is exposed:

- a visible search input surfaced as `文本框 搜索`
- the search shell can sometimes be observed as `面板 搜索`
- the close button surfaced as `按钮 关闭`

What is not proven:

- readable `AXValue` for the QQ Music search field
- settable value support for the search field
- search-result rows as stable AX nodes
- result tabs or visible `播放 / 下载 / 批量` controls as reliable AX actions

The strongest negative evidence from the probe was:

`Apple event error -10005: Cannot set a value for an element that is not settable`

Current conclusion:

- AX observation is useful for revealing and locating the search entry surface
- AX value-setting is not a safe dependency for QQ Music search input
- AX cannot currently be treated as the primary signal for result selection

## Proven Control Paths

### Search Entry Phase

The current probe supports a keyboard-first interpretation of QQ Music search
entry.

Proven facts:

- `Cmd+F` opens the search surface
- ASCII key input can visibly commit into the search field
- `Return` can submit the query
- `Down + Return` can accept a visible suggestion

This is enough to support a search-entry recipe that is pointer-free.

### Result Selection Phase

The current probe does not prove a stable pointer-free result-selection path.

Observed failures or weak points:

- result rows were not stably represented as AX nodes
- `Tab`, `Down`, and `Escape` on the result page did not establish a reliable
  keyboard navigation loop
- coordinate clicks on rows, titles, covers, and the top `播放` pill did not
  produce a deterministic selection path
- one sampled click on a `播放`-like target returned `AXError.noValue`

Current conclusion:

- stable concrete result selection still requires OCR / pointer fallback
- a single generic semantic result-selection path is still not proven
- however, one narrow playback activation baseline is now validated:
  constrained OCR anchor -> row double-click -> captured evidence image OCR

## Disturbance Classification

QQ Music should not currently be advertised as non-disturbing automation.

The right interpretation is phase-specific:

- search open / input / submit: `keyboard`
- internal search focus changes: `focus`
- concrete result selection: `pointer`

The app did not become the sampled frontmost OS app during the probe, but that
fact alone is not enough to claim that the path is truly non-disturbing. Future
reports should continue to distinguish:

- pointer disturbance
- keyboard disturbance
- focus disturbance
- clipboard disturbance
- foreground-app disturbance

## Input Implications

The current `type_text` path is acceptable for ASCII baselines, but Chinese
input is not yet reliable.

Probe result:

- `type_text 周杰伦` did not visibly commit in the sampled session

Current implication:

- AUV should not assume IME-safe text entry from the current keystroke path
- a paste-aware input primitive is likely the next useful input addition

Recommended primitive split:

- `type_text_keystroke`
- `paste_text_preserve_clipboard`
- `ax_set_value`

These should remain separate. They have different guarantees and different
disturbance classes.

### Follow-up Validation: Clipboard-Backed Chinese Search Entry

An additional validation pass on `2026-05-15` confirmed that:

- `debug.pressKey --key cmd+f` followed by
  `debug.pasteTextPreserveClipboard --text 周杰伦 --submit_key return`
  successfully reaches a QQ音乐 result page for `周杰伦`
- the search-entry path preserves a TextEdit sentinel and restores the textual
  clipboard contents after the paste/submit sequence
- the resulting screenshot clearly shows a QQ音乐 search results page for
  `周杰伦`

What this does **not** prove:

- `debug.findScreenText --query 周杰伦` is not currently validated
- `debug.findScreenText --query 晴天` is not currently validated
- stable Chinese OCR anchor resolution should therefore still be treated as
  unproven

This means the current honest claim is:

- Chinese query submission is validated through clipboard-backed search entry
- Chinese result selection through OCR is not yet validated

## Recommended Recipe Boundary

The first honest QQ Music recipe boundary is:

`open search -> input query -> submit query`

Optionally followed by:

`resolve OCR anchor -> click OCR anchor -> capture evidence`

And more narrowly, a later validation slice can currently claim:

`resolve OCR anchor -> double-click visible row -> verify player title from captured evidence image`

The current repo should not yet claim a recipe like:

- `qqmusic.play_song`
- `qqmusic.select_first_result_and_play`

Those names imply a result-selection and playback contract that has not yet
been proven.

## Verdict

QQ Music macOS `11.1.1` exposes a partial AX surface over its search shell, but
its result surface behaves like an AX-weak embedded `WKWebView`.

Therefore:

- pointer-free search entry is feasible
- `AX set value` is not a safe dependency for search input
- stable concrete result selection is not proven without OCR / pointer fallback
- the first validated playback path is a result-row double-click followed by
  OCR verification over the captured post-click evidence image
- the app must not currently be described as fully semantic-controllable or
  non-disturbing

The most accurate short form is:

`keyboard-first search entry, pointer fallback required for stable result selection`

The most accurate extended form is:

`keyboard-first search entry, OCR/pointer fallback for result selection, and a narrow double-click playback baseline verified from captured evidence`

## Implementation Gates

The next code changes should follow this order:

1. Add disturbance metadata to primitives and recipes.
2. Add a paste-aware input primitive that preserves and restores the clipboard.
3. Keep the first QQ Music recipe scoped to search entry and evidence capture.
4. Treat broader result selection and broader playback as later milestones, even
   though one narrow double-click playback baseline is now validated.

## Allowed Claims

The current state is strong enough to claim:

- QQ Music has a validated keyboard-first search-entry path on macOS.
- AUV can already validate a `search -> OCR anchor resolve -> OCR anchor click -> evidence capture` slice.
- QQ Music result selection currently depends on OCR / pointer fallback.
- AUV can validate one narrow playback slice through
  `search -> OCR anchor -> row double-click -> captured-image OCR verification`.

## Forbidden Claims

The current state is not strong enough to claim:

- QQ Music has a broad validated playback skill.
- QQ Music result rows are fully controllable through AX.
- the current workflow is non-disturbing.
- a lightweight model can already complete the full playback task through a
  stable general-purpose high-level skill.

## Follow-up Validation: Narrow Playback Baseline

An additional validation pass on `2026-05-15` established one narrow playback
baseline:

- query: `aa`
- visible result-row anchor: `Cure For Me`
- activation path: row double-click
- verification signal: OCR over the captured post-click evidence image
- verified player-title query: `Cure For Me - AURORA`

What is currently strong:

- the row double-click path can change QQ音乐 into a visible now-playing state
- the post-click screenshot can be inspected without recapturing the live
  desktop
- the player-title region in the captured evidence image can confirm the
  expected title through `debug.findImageText`

What is still not proven:

- a generalized playback activation strategy for arbitrary rows
- a pointer-free playback activation strategy
- reliable live-screen OCR verification for the bottom player title region
- Chinese OCR anchor resolution for result selection

This matters because the current playback baseline is real, but narrow. It
should be treated as a validated exploratory skill slice, not a general QQ音乐
playback contract.

## Related Files

- `docs/ai/references/apps/qqmusic/2026-05-14-glm-air-qqmusic-search-evidence.md`
- `docs/ai/references/2026-05-14-qqmusic-search-ocr-anchor-skill-v0.json`
- `docs/ai/references/2026-05-15-qqmusic-play-visible-anchor-skill-v0.json`
- `docs/ai/references/apps/qqmusic/2026-05-15-qqmusic-playback-verification.md`
- `docs/ai/references/evidence/2026-05-14-qqmusic-search-ocr-anchor/`
- `recipes/macos/qqmusic/search-ocr-anchor.v0.json`
- `recipes/macos/qqmusic/play-visible-anchor.v0.json`
